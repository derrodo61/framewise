use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn tags(value: &Value) -> BTreeMap<String, String> {
    value["tags"]
        .as_object()
        .map(|tags| {
            tags.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn needs_metadata_keys(tags: &BTreeMap<String, String>) -> bool {
    tags.keys().any(|key| {
        ![
            "major_brand",
            "minor_version",
            "compatible_brands",
            "creation_time",
            "title",
            "comment",
            "encoder",
            "duration",
            "bit_rate",
            "number_of_frames",
        ]
        .iter()
        .any(|standard| key.eq_ignore_ascii_case(standard))
    })
}

pub(super) fn preserved_tag(actual: Option<&String>, expected: &str, key: &str) -> bool {
    actual.is_some_and(|value| {
        value == expected
            || (key.eq_ignore_ascii_case("creation_time")
                && value.split(';').count() > 1
                && value.split(';').all(|part| part == expected))
    })
}

pub(super) fn changed_stream_tags(
    label: &str,
    source: &BTreeMap<String, String>,
    output: &BTreeMap<String, String>,
) -> Option<String> {
    let changed: Vec<&str> = source
        .iter()
        .filter(|(key, value)| {
            !key.eq_ignore_ascii_case("encoder") && !preserved_tag(output.get(*key), value, key)
        })
        .map(|(key, _)| key.as_str())
        .collect();
    (!changed.is_empty()).then(|| format!("{label}: {}", changed.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::{changed_stream_tags, needs_metadata_keys, preserved_tag};
    use std::collections::BTreeMap;

    #[test]
    fn standard_mp4_tags_do_not_need_generic_metadata_keys() {
        let mut tags = BTreeMap::from([
            ("creation_time".into(), "2026-10-04T18:55:09.000000Z".into()),
            ("title".into(), "Recording".into()),
            ("comment".into(), "Captured with Snagit".into()),
        ]);
        assert!(!needs_metadata_keys(&tags));
        tags.insert("camera_model".into(), "Example".into());
        assert!(needs_metadata_keys(&tags));
    }

    #[test]
    fn duplicate_mp4_creation_time_values_still_preserve_the_date() {
        let expected = "2026-10-04T18:55:09.000000Z";
        assert!(preserved_tag(
            Some(&expected.to_string()),
            expected,
            "creation_time"
        ));
        assert!(preserved_tag(
            Some(&format!("{expected};{expected}")),
            expected,
            "creation_time"
        ));
        assert!(!preserved_tag(
            Some(&format!("{expected};2026-10-05T18:55:09.000000Z")),
            expected,
            "creation_time"
        ));
    }

    #[test]
    fn reports_changed_track_tags_but_not_the_new_encoder() {
        let source = BTreeMap::from([
            ("creation_time".into(), "earlier".into()),
            ("encoder".into(), "old encoder".into()),
            ("name".into(), "Camera".into()),
        ]);
        let output = BTreeMap::from([
            ("creation_time".into(), "later".into()),
            ("encoder".into(), "new encoder".into()),
        ]);
        assert_eq!(
            changed_stream_tags("Video", &source, &output).as_deref(),
            Some("Video: creation_time, name")
        );
    }
}
