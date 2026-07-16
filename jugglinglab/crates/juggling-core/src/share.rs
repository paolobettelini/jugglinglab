use crate::animation_prefs::AnimationPrefs;
use crate::jml::{PatternRecord, parse_jml, record_to_pattern_jml};
use crate::parameter_list::ParameterList;
use crate::siteswap;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{Read, Write};

const ALLOWED_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~=;&";

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedShare {
    pub record: PatternRecord,
    pub prefs: Option<AnimationPrefs>,
}

pub fn build_share_url(
    base_url: &str,
    record: &PatternRecord,
    prefs: &AnimationPrefs,
) -> Result<String, String> {
    let pattern_config = if record
        .notation
        .as_deref()
        .is_some_and(|notation| notation.eq_ignore_ascii_case("siteswap"))
        && record.raw_pattern.is_none()
    {
        record.config.clone()
    } else {
        None
    };

    let pattern_config = match pattern_config {
        Some(config) => config,
        None => {
            let xml = record_to_pattern_jml(record)?;
            let compressed = gzip_compress(xml.as_bytes())?;
            format!("jml={}", BASE64_STANDARD.encode(compressed))
        }
    };
    let prefs_config = prefs.to_string();
    let full_config = if prefs_config.is_empty() {
        pattern_config
    } else {
        format!("{pattern_config};{prefs_config}")
    };
    Ok(format!(
        "{}?{}",
        base_url.trim_end_matches('?'),
        url_encode(&full_config)
    ))
}

pub fn decode_share_url(url: &str) -> Result<DecodedShare, String> {
    let encoded = url.split_once('?').map_or(url, |(_, query)| query);
    let mut settings = url_decode(encoded)?;
    if !settings.contains('=') {
        settings = format!("pattern={settings}");
    }

    let mut parameters = ParameterList::parse(Some(&settings))?;
    let jml_data = parameters.remove_parameter("jml");
    let prefs = AnimationPrefs::from_parameters(&mut parameters)?;
    let prefs = (prefs != AnimationPrefs::default()).then_some(prefs);

    let record = match jml_data.filter(|value| !value.trim().is_empty()) {
        Some(encoded) => {
            let compressed = BASE64_STANDARD
                .decode(encoded)
                .map_err(|error| format!("Invalid shared JML Base64 data: {error}"))?;
            let xml = gzip_decompress(&compressed)?;
            let library = parse_jml(&xml)?;
            library
                .records
                .into_iter()
                .find(PatternRecord::is_playable)
                .ok_or_else(|| "Shared JML does not contain a playable pattern".to_string())?
        }
        None => {
            let config = parameters.to_string();
            let spec = siteswap::parse_config(&config)?;
            PatternRecord::siteswap(siteswap::display_title(&spec), config)
        }
    };

    Ok(DecodedShare { record, prefs })
}

pub fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 2);
    for character in input.chars() {
        if character.is_ascii() && ALLOWED_CHARS.contains(character) {
            encoded.push(character);
            continue;
        }
        let mut bytes = [0_u8; 4];
        for byte in character.encode_utf8(&mut bytes).as_bytes() {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

pub fn url_decode(input: &str) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(input.len());
    let mut characters = input.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character == '%' && index + 2 < input.len() {
            let source = input.as_bytes();
            let high = source[index + 1];
            let low = source[index + 2];
            if high.is_ascii_hexdigit() && low.is_ascii_hexdigit() {
                let byte = (hex_value(high) << 4) | hex_value(low);
                bytes.push(byte);
                characters.next();
                characters.next();
                continue;
            }
        }
        let mut encoded = [0_u8; 4];
        bytes.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
    }
    String::from_utf8(bytes).map_err(|error| format!("Invalid UTF-8 in shared URL: {error}"))
}

fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

fn gzip_compress(input: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(input)
        .map_err(|error| format!("Unable to compress shared JML: {error}"))?;
    encoder
        .finish()
        .map_err(|error| format!("Unable to finish shared JML compression: {error}"))
}

fn gzip_decompress(input: &[u8]) -> Result<String, String> {
    let mut decoder = GzDecoder::new(input);
    let mut xml = String::new();
    decoder
        .read_to_string(&mut xml)
        .map_err(|error| format!("Unable to decompress shared JML: {error}"))?;
    Ok(xml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encoding_matches_the_original_allowed_character_set() {
        let source = "pattern=(4,2x)*;title=Café & pass";
        let encoded = url_encode(source);

        assert_eq!(
            encoded,
            "pattern=%284%2C2x%29%2A;title=Caf%C3%A9%20&%20pass"
        );
        assert_eq!(url_decode(&encoded).unwrap(), source);
    }

    #[test]
    fn siteswap_share_round_trips_preferences_without_jml_payload() {
        let record = PatternRecord::siteswap("Cascade", "pattern=3;dwell=1.2");
        let prefs = AnimationPrefs {
            stereo: true,
            slowdown: 1.5,
            ..AnimationPrefs::default()
        };

        let url = build_share_url("https://jugglinglab.org/anim", &record, &prefs).unwrap();
        assert!(url.starts_with("https://jugglinglab.org/anim?pattern=3;dwell=1.2;"));
        assert!(!url.contains("jml="));

        let decoded = decode_share_url(&url).unwrap();
        assert_eq!(decoded.record.config, record.config);
        assert_eq!(decoded.prefs, Some(prefs));
    }

    #[test]
    fn jml_share_uses_base64_gzip_and_round_trips_the_pattern() {
        let mut record = PatternRecord::siteswap("Cascade", "pattern=3");
        record.notation = Some("jml".to_string());
        record.raw_pattern = Some(
            "<pattern><title>Edited</title><setup jugglers=\"1\" paths=\"1\"/></pattern>"
                .to_string(),
        );

        let url = build_share_url(
            "https://jugglinglab.org/anim",
            &record,
            &AnimationPrefs::default(),
        )
        .unwrap();
        assert!(url.contains("?jml="));

        let decoded = decode_share_url(&url).unwrap();
        assert_eq!(decoded.record.display, "Edited");
        assert!(
            decoded
                .record
                .raw_pattern
                .as_deref()
                .is_some_and(|xml| xml.contains("<title>Edited</title>"))
        );
        assert_eq!(decoded.prefs, None);
    }

    #[test]
    fn simplified_share_query_is_treated_as_a_pattern() {
        let decoded = decode_share_url("https://jugglinglab.org/anim?531").unwrap();
        assert_eq!(decoded.record.config.as_deref(), Some("pattern=531"));
    }
}
