//! Serde 序列化契约辅助模块
//! 遵循 `CODING_STANDARDS.md` §4.3 规范

/// 大整数序列化为字符串 (u64/i64/u128/i128)
pub mod string {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::fmt::Display;
    use std::str::FromStr;

    pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Display + Serialize,
        S: Serializer,
    {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: FromStr + Deserialize<'de>,
        <T as FromStr>::Err: Display,
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        T::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// 字节数组 Hex 编码序列化 ([u8; N] / Vec<u8>)
pub mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::fmt::Write;

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut hex_string = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            write!(hex_string, "{b:02x}").map_err(serde::ser::Error::custom)?;
        }
        serializer.serialize_str(&hex_string)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(serde::de::Error::custom))
            .collect()
    }
}
