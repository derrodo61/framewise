//! Read PNG text chunks without decoding pixels or depending on ffprobe.
use std::{collections::BTreeMap, fs::File, io::{self, Read, Seek, SeekFrom}, path::Path};
use flate2::read::ZlibDecoder;
use serde_json::{Value, json};

const MAX_TEXT: usize = 16 * 1024 * 1024;
fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
fn split(data: &[u8]) -> io::Result<(&[u8], &[u8])> {
    let end = data.iter().position(|byte| *byte == 0).ok_or_else(|| invalid("Invalid text chunk"))?;
    Ok((&data[..end], &data[end + 1..]))
}
fn latin1(data: &[u8]) -> String { data.iter().map(|byte| char::from(*byte)).collect() }
fn inflate(data: &[u8], limit: usize) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    ZlibDecoder::new(data).take(limit as u64 + 1).read_to_end(&mut output)?;
    if output.len() > limit { return Err(invalid("PNG text metadata exceeds 16 MiB")); }
    Ok(output)
}
fn text(kind: &[u8; 4], data: &[u8], limit: usize) -> io::Result<(String, String)> {
    let (keyword, rest) = split(data)?;
    if keyword.is_empty() || keyword.len() > 79 { return Err(invalid("Invalid PNG text keyword")); }
    let value = match kind {
        b"tEXt" => latin1(rest),
        b"zTXt" => {
            if rest.first() != Some(&0) { return Err(invalid("Unsupported PNG text compression")); }
            latin1(&inflate(&rest[1..], limit)?)
        }
        b"iTXt" => {
            if rest.len() < 2 || rest[0] > 1 || rest[1] != 0 { return Err(invalid("Invalid international text chunk")); }
            let (_, translated) = split(&rest[2..])?;
            let (_, value) = split(translated)?;
            let bytes = if rest[0] == 1 { inflate(value, limit)? } else { value.to_vec() };
            String::from_utf8(bytes).map_err(|_| invalid("Invalid UTF-8 PNG text"))?
        }
        _ => unreachable!(),
    };
    Ok((latin1(keyword), value))
}

pub(crate) fn inspect(path: &Path) -> io::Result<Value> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut signature = [0; 8];
    file.read_exact(&mut signature)?;
    if signature != *b"\x89PNG\r\n\x1a\n" { return Err(invalid("Invalid PNG signature")); }
    let mut tags = BTreeMap::new();
    let mut dimensions = None;
    let mut budget = MAX_TEXT;
    loop {
        let mut header = [0; 8];
        file.read_exact(&mut header)?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = header[4..].try_into().unwrap();
        if length as u64 + 4 > size.saturating_sub(file.stream_position()?) { return Err(invalid("Truncated PNG chunk")); }
        let is_text = matches!(&kind, b"tEXt" | b"zTXt" | b"iTXt");
        if is_text || &kind == b"IHDR" {
            if (is_text && length > budget) || (&kind == b"IHDR" && length != 13) { return Err(invalid("Invalid or oversized PNG metadata")); }
            let mut data = vec![0; length];
            file.read_exact(&mut data)?;
            let mut crc = [0; 4];
            file.read_exact(&mut crc)?;
            let mut hash = crc32fast::Hasher::new();
            hash.update(&kind); hash.update(&data);
            if hash.finalize() != u32::from_be_bytes(crc) { return Err(invalid("PNG metadata checksum mismatch")); }
            if is_text {
                let (key, value) = text(&kind, &data, budget)?;
                budget = budget.checked_sub(length.max(value.len())).ok_or_else(|| invalid("PNG text metadata exceeds 16 MiB"))?;
                tags.insert(key, value);
            } else {
                let width = u32::from_be_bytes(data[..4].try_into().unwrap());
                let height = u32::from_be_bytes(data[4..8].try_into().unwrap());
                if width == 0 || height == 0 { return Err(invalid("Invalid PNG dimensions")); }
                dimensions = Some((width, height));
            }
        } else { file.seek(SeekFrom::Current(length as i64 + 4))?; }
        if &kind == b"IEND" { break; }
    }
    let (width, height) = dimensions.ok_or_else(|| invalid("PNG has no image header"))?;
    Ok(json!({
        "format": { "format_name": "png", "format_long_name": "PNG image", "size": size.to_string(), "tags": tags },
        "streams": [{ "codec_type": "video", "codec_name": "png", "width": width, "height": height }]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn compressed(value: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(value).unwrap(); encoder.finish().unwrap()
    }
    #[test]
    fn all_png_text_encodings() {
        assert_eq!(text(b"tEXt", b"prompt\0caf\xe9", MAX_TEXT).unwrap().1, "caf\u{e9}");
        let mut z = b"prompt\0\0".to_vec(); z.extend(compressed(b"{\"seed\":18446744073709551615}"));
        assert_eq!(text(b"zTXt", &z, MAX_TEXT).unwrap().1, "{\"seed\":18446744073709551615}");
        let mut i = b"workflow\0\x01\0en\0translated\0".to_vec(); i.extend(compressed("woman \u{2192} portrait".as_bytes()));
        assert_eq!(text(b"iTXt", &i, MAX_TEXT).unwrap().1, "woman \u{2192} portrait");
        assert_eq!(text(b"iTXt", b"prompt\0\0\0\0\0hello", MAX_TEXT).unwrap().1, "hello");
        assert!(text(b"zTXt", &z, 4).is_err());
        assert!(text(b"iTXt", b"prompt\0\x02\0", MAX_TEXT).is_err());
        assert!(text(b"tEXt", b"no separator", MAX_TEXT).is_err());
    }
    #[test]
    fn reads_text_after_pixels_and_rejects_corruption() {
        fn chunk(bytes: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            bytes.extend((data.len() as u32).to_be_bytes()); bytes.extend(kind); bytes.extend(data);
            let mut hash = crc32fast::Hasher::new(); hash.update(kind); hash.update(data);
            bytes.extend(hash.finalize().to_be_bytes());
        }
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut bytes, b"IHDR", &[0,0,3,0,0,0,4,0,8,2,0,0,0]);
        chunk(&mut bytes, b"IDAT", &[]);
        chunk(&mut bytes, b"tEXt", b"prompt\0{\"seed\":18446744073709551615}");
        chunk(&mut bytes, b"IEND", &[]);
        let path = std::env::temp_dir().join(format!("framewise-png-metadata-{}.png", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let metadata = inspect(&path).unwrap();
        assert_eq!(metadata["streams"][0]["width"], 768);
        assert_eq!(metadata["streams"][0]["height"], 1024);
        assert_eq!(metadata["format"]["tags"]["prompt"], "{\"seed\":18446744073709551615}");
        bytes[29] ^= 1;
        std::fs::write(&path, &bytes).unwrap(); assert!(inspect(&path).is_err());
        std::fs::write(&path, &bytes[..20]).unwrap(); assert!(inspect(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
