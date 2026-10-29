//! Preserve image generation metadata that ffprobe does not expose.
use std::{fs::File, io::BufReader, path::Path};
use exif::{In, Reader, Tag, Value};

fn user_comment(data: &[u8]) -> Result<String, String> {
    let text = if let Some(bytes) = data.strip_prefix(b"UNICODE\0") {
        // WAN2GP writes UTF-16LE without a BOM, independently of the TIFF byte order.
        // Accept BOMs and recognizable big-endian JSON from other writers as well.
        let (bytes, little_endian) = if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
            (rest, true)
        } else if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
            (rest, false)
        } else { (bytes, !bytes.starts_with(&[0, b'{']) && !bytes.starts_with(&[0, b'['])) };
        if bytes.len() % 2 != 0 { return Err("Invalid UTF-16 EXIF UserComment length".into()); }
        let words: Vec<_> = bytes.as_chunks::<2>().0.iter().map(|pair| {
            let pair = *pair;
            if little_endian { u16::from_le_bytes(pair) } else { u16::from_be_bytes(pair) }
        }).collect();
        String::from_utf16(&words).map_err(|_| "Invalid UTF-16 EXIF UserComment")?
    } else {
        if data.starts_with(b"JIS\0\0\0\0\0") { return Err("Unsupported JIS EXIF UserComment encoding".into()); }
        let bytes = data.strip_prefix(b"ASCII\0\0\0").or_else(|| data.strip_prefix(&[0; 8])).unwrap_or(data);
        std::str::from_utf8(bytes).map_err(|_| "Invalid UTF-8/ASCII EXIF UserComment")?.to_owned()
    };
    Ok(text.trim_matches('\0').trim_start_matches('\u{feff}').to_owned())
}

pub(crate) fn enrich(path: &Path, metadata: &mut serde_json::Value) -> Result<(), String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let exif = match Reader::new().continue_on_error(true)
        .read_from_container(&mut BufReader::new(file))
        .or_else(|error| error.distill_partial_result(|errors| {
            // Some piexif writers omit a sub-IFD terminator. Keep usable fields.
            log::warn!("Recovered image EXIF metadata with {} non-fatal parsing warning(s)", errors.len());
        })) {
        Ok(exif) => exif,
        Err(exif::Error::NotFound(_)) => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let mut tags = serde_json::Map::new();
    if let Some(field) = exif.get_field(Tag::UserComment, In::PRIMARY) {
        let value = match &field.value {
            Value::Undefined(bytes, _) | Value::Byte(bytes) => user_comment(bytes)?,
            Value::Ascii(strings) => user_comment(strings.first().map_or(&[], Vec::as_slice))?,
            _ => return Err("Unsupported EXIF UserComment value".into()),
        };
        if !value.trim().is_empty() { tags.insert("UserComment".into(), value.into()); }
    }
    if let Some(field) = exif.get_field(Tag::ImageDescription, In::PRIMARY)
        && let Value::Ascii(strings) = &field.value
        && let Some(bytes) = strings.first()
        && let Ok(value) = std::str::from_utf8(bytes)
        && !value.trim().is_empty() {
        tags.insert("description".into(), value.into());
    }
    if !tags.is_empty() {
        if !metadata["format"].is_object() { metadata["format"] = serde_json::json!({}); }
        if !metadata["format"]["tags"].is_object() { metadata["format"]["tags"] = serde_json::json!({}); }
        metadata["format"]["tags"].as_object_mut().ok_or("Invalid metadata tags")?.extend(tags);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_wan2gp_ascii_unicode_and_preserves_seed_digits() {
        assert_eq!(user_comment(b"ASCII\0\0\0{\"seed\":18446744073709551615}\0").unwrap(), "{\"seed\":18446744073709551615}");
        let prompt = "{\"prompt\":\"Caf\u{e9} \u{732b} \u{1f30a}\"}";
        for (little, bom) in [(true,false),(true,true),(false,true),(false,false)] {
            let mut bytes = b"UNICODE\0".to_vec();
            if bom { bytes.extend(if little { [0xff,0xfe] } else { [0xfe,0xff] }); }
            for word in prompt.encode_utf16() { bytes.extend(if little { word.to_le_bytes() } else { word.to_be_bytes() }); }
            assert_eq!(user_comment(&bytes).unwrap(), prompt);
        }
        assert_eq!(user_comment(b"\0\0\0\0\0\0\0\0plain UTF-8").unwrap(), "plain UTF-8");
        assert!(user_comment(b"UNICODE\0x").is_err());
        assert!(user_comment(b"JIS\0\0\0\0\0text").is_err());
    }
    #[test]
    fn reads_jpeg_and_webp_user_comments_without_changing_other_metadata() {
        for (name, expected) in [("wan2gp-qwen-image.jpg", "A red sailboat on a calm lake."), ("wan2gp-qwen-image.webp", "Caf\u{e9} by the lake \u{732b}")] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/fixtures").join(name);
            // Fixtures retain WAN2GP/piexif's missing sub-IFD terminator.
            assert!(Reader::new().read_from_container(&mut BufReader::new(File::open(&path).unwrap())).is_err());
            let mut metadata = serde_json::json!({ "format": { "tags": { "existing": "retained" } }, "streams": [{ "width": 16 }] });
            enrich(&path, &mut metadata).unwrap();
            assert_eq!(metadata["format"]["tags"]["existing"], "retained");
            assert_eq!(metadata["streams"][0]["width"], 16);
            let text = metadata["format"]["tags"]["UserComment"].as_str().unwrap();
            assert!(text.contains("18446744073709551615"));
            let generation: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(generation["prompt"], expected);
            assert!(generation["type"].as_str().unwrap().starts_with("WanGP"));
        }
    }
}
