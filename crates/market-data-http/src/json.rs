use crate::{HttpError, HttpErrorKind as K};
use edgeagent_market_data_domain::{ShareVolume, UsdPrice};
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use std::collections::BTreeSet;

/// Valid bounded JSON with exact raw numeric tokens and no duplicate keys at any depth.
pub struct JsonDocument {
    root: Box<RawValue>,
    bytes: Vec<u8>,
}
impl std::fmt::Debug for JsonDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JsonDocument(..)")
    }
}
impl JsonDocument {
    /// Validate at most 1 MiB, 32 nested containers and 65,536 values before provider mapping.
    /// No floating-point conversion occurs; malformed JSON is never repaired.
    /// # Errors
    /// Returns a payload-free InvalidJson category for syntax, duplication or capacity failures.
    pub fn parse(bytes: &[u8]) -> Result<Self, HttpError> {
        if bytes.len() > 1_048_576 {
            return Err(K::InvalidJson.into());
        }
        Self::from_body(bytes.to_vec())
    }
    pub(crate) fn from_body(bytes: Vec<u8>) -> Result<Self, HttpError> {
        if bytes.len() > 1_048_576 {
            return Err(K::InvalidJson.into());
        }
        let root: Box<RawValue> = serde_json::from_slice(&bytes).map_err(|_| K::InvalidJson)?;
        validate(&root, 0, &mut 65_536)?;
        Ok(Self { root, bytes })
    }
    /// Exact validated JSON; provider DTOs must borrow RawValue fields, not floats or Value numbers.
    #[must_use]
    pub fn root(&self) -> &RawValue {
        &self.root
    }
    /// Exact identity-encoded response body, including outer whitespace, for authorized checksums.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
fn validate(raw: &RawValue, depth: u8, remaining: &mut usize) -> Result<(), HttpError> {
    *remaining = remaining.checked_sub(1).ok_or(K::InvalidJson)?;
    if depth > 32 {
        return Err(K::InvalidJson.into());
    }
    if matches!(raw.get().as_bytes().first(), Some(b'{' | b'[')) {
        let mut decoder = serde_json::Deserializer::from_str(raw.get());
        Container { depth, remaining }
            .deserialize(&mut decoder)
            .map_err(|_| K::InvalidJson)?;
        decoder.end().map_err(|_| K::InvalidJson)?;
    }
    Ok(())
}
struct Container<'a> {
    depth: u8,
    remaining: &'a mut usize,
}
impl<'de> DeserializeSeed<'de> for Container<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        decoder.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Container<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bounded JSON container")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(serde::de::Error::custom("duplicate JSON key"));
            }
            let child: &RawValue = map.next_value()?;
            validate(child, self.depth + 1, self.remaining)
                .map_err(|_| serde::de::Error::custom("invalid bounded JSON"))?;
        }
        Ok(())
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<(), S::Error> {
        while let Some(child) = sequence.next_element::<&RawValue>()? {
            validate(child, self.depth + 1, self.remaining)
                .map_err(|_| serde::de::Error::custom("invalid bounded JSON"))?;
        }
        Ok(())
    }
}
/// Convert an exact JSON number or numeric string to a positive USD/share price.
/// # Errors
/// Rejects malformed, overflowing or excess-scale tokens; never rounds or passes through f64.
pub fn price(raw: &RawValue) -> Result<UsdPrice, HttpError> {
    let (coefficient, scale) = decimal(raw, 8)?;
    UsdPrice::new(coefficient, scale).map_err(|_| K::InvalidJson.into())
}
/// Convert an exact JSON number or numeric string to nonnegative reported shares.
/// # Errors
/// Rejects malformed, negative, overflowing or excess-scale tokens without rounding.
pub fn volume(raw: &RawValue) -> Result<ShareVolume, HttpError> {
    let (coefficient, scale) = decimal(raw, 6)?;
    ShareVolume::new(coefficient, scale).map_err(|_| K::InvalidJson.into())
}
fn decimal(raw: &RawValue, max_scale: i32) -> Result<(i128, u32), HttpError> {
    if raw.get().len() > 128 {
        return Err(K::InvalidJson.into());
    }
    let text = if raw.get().starts_with('"') {
        serde_json::from_str::<String>(raw.get()).map_err(|_| K::InvalidJson)?
    } else {
        raw.get().to_owned()
    };
    // Reuse serde's JSON grammar, including exponent syntax, without decoding a number.
    let number: &RawValue = serde_json::from_str(&text).map_err(|_| K::InvalidJson)?;
    let text = number.get();
    if !text.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return Err(K::InvalidJson.into());
    }
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or(Ok((text, 0)), |(m, e)| {
            e.parse::<i32>()
                .ok()
                .filter(|e| (-128..=128).contains(e))
                .map(|e| (m, e))
                .ok_or(K::InvalidJson)
        })?;
    let fractional = mantissa.split_once('.').map_or(0, |(_, f)| f.len() as i32);
    let scale = fractional - exponent;
    if scale > max_scale {
        return Err(K::InvalidJson.into());
    }
    let mut digits = mantissa.replace('.', "");
    if scale < 0 {
        if digits.trim_start_matches('0').len() + (-scale) as usize > 28 {
            return Err(K::InvalidJson.into());
        }
        digits.extend(std::iter::repeat_n('0', (-scale) as usize));
    }
    let coefficient = digits.parse::<i128>().map_err(|_| K::InvalidJson)?;
    Ok((coefficient, scale.max(0) as u32))
}
