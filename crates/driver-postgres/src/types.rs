use std::collections::HashMap;
use tokio_postgres::types::Type;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomTypeInfo {
    pub oid: u32,
    pub name: String,
    pub schema: String,
    pub typtype: char, // 'e' = enum, 'c' = composite, 'd' = domain, 'b' = base
    pub typcategory: char,
    pub typelem: u32,
    pub typbasetype: u32,
    pub base_type_name: Option<String>,
    pub full_type_name: String,
    pub description: Option<String>,
}

#[derive(Default)]
pub struct CustomTypeRegistry {
    pub types: HashMap<u32, CustomTypeInfo>,
    pub by_name: HashMap<String, Vec<CustomTypeInfo>>,
}

impl CustomTypeRegistry {
    pub fn new() -> Self {
        Self {
            types: HashMap::new(),
            by_name: HashMap::new(),
        }
    }

    pub fn insert(&mut self, info: CustomTypeInfo) {
        self.by_name
            .entry(info.name.clone())
            .or_default()
            .push(info.clone());
        self.types.insert(info.oid, info);
    }

    pub fn get_by_oid(&self, oid: u32) -> Option<&CustomTypeInfo> {
        self.types.get(&oid)
    }

    pub fn resolve_by_name(&self, name: &str, search_path: &[String]) -> Option<&CustomTypeInfo> {
        if name.contains('.') {
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() == 2 {
                let schema = parts[0];
                let typname = parts[1];
                if let Some(infos) = self.by_name.get(typname) {
                    return infos.iter().find(|info| info.schema == schema);
                }
            }
        } else if let Some(infos) = self.by_name.get(name) {
            for schema in search_path {
                if let Some(info) = infos.iter().find(|info| info.schema == *schema) {
                    return Some(info);
                }
            }
        }
        None
    }
}

pub struct RawValue<'a>(pub &'a [u8]);

impl<'a> tokio_postgres::types::FromSql<'a> for RawValue<'a> {
    fn from_sql(
        _ty: &tokio_postgres::types::Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(RawValue(raw))
    }

    fn accepts(_ty: &tokio_postgres::types::Type) -> bool {
        true
    }
}

pub fn decode_hstore(raw: &[u8]) -> Result<serde_json::Value, String> {
    if raw.len() < 4 {
        return Err("Invalid hstore header".to_string());
    }
    let num_pairs = u32::from_be_bytes(raw[0..4].try_into().unwrap()) as usize;
    let mut map = serde_json::Map::new();
    let mut cursor = 4;
    for _ in 0..num_pairs {
        if cursor + 4 > raw.len() {
            return Err("Unexpected end of hstore".to_string());
        }
        let key_len = i32::from_be_bytes(raw[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;
        if key_len < 0 {
            return Err("Invalid hstore key length".to_string());
        }
        let k_len = key_len as usize;
        if cursor + k_len > raw.len() {
            return Err("Hstore key overflow".to_string());
        }
        let key = String::from_utf8_lossy(&raw[cursor..cursor + k_len]).into_owned();
        cursor += k_len;

        if cursor + 4 > raw.len() {
            return Err("Unexpected end of hstore value".to_string());
        }
        let val_len_i32 = i32::from_be_bytes(raw[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;

        let val = if val_len_i32 < 0 {
            serde_json::Value::Null
        } else {
            let v_len = val_len_i32 as usize;
            if cursor + v_len > raw.len() {
                return Err("Hstore value overflow".to_string());
            }
            let value_str = String::from_utf8_lossy(&raw[cursor..cursor + v_len]).into_owned();
            cursor += v_len;
            serde_json::Value::String(value_str)
        };

        map.insert(key, val);
    }
    Ok(serde_json::Value::Object(map))
}

pub fn decode_interval(raw: &[u8]) -> Result<serde_json::Value, String> {
    if raw.len() < 16 {
        return Err("Invalid interval length".to_string());
    }
    let microseconds = i64::from_be_bytes(raw[0..8].try_into().unwrap());
    let days = i32::from_be_bytes(raw[8..12].try_into().unwrap());
    let months = i32::from_be_bytes(raw[12..16].try_into().unwrap());

    let years = months / 12;
    let remaining_months = months % 12;

    let hours = microseconds / 3_600_000_000;
    let remaining_micros = microseconds % 3_600_000_000;
    let minutes = remaining_micros / 60_000_000;
    let remaining_micros = remaining_micros % 60_000_000;
    let seconds = (remaining_micros as f64) / 1_000_000.0;

    let mut parts = Vec::new();
    if years != 0 {
        parts.push(format!(
            "{} year{}",
            years,
            if years.abs() == 1 { "" } else { "s" }
        ));
    }
    if remaining_months != 0 {
        parts.push(format!(
            "{} month{}",
            remaining_months,
            if remaining_months.abs() == 1 { "" } else { "s" }
        ));
    }
    if days != 0 {
        parts.push(format!(
            "{} day{}",
            days,
            if days.abs() == 1 { "" } else { "s" }
        ));
    }
    if hours != 0 || minutes != 0 || seconds != 0.0 {
        parts.push(format!("{:02}:{:02}:{:06.3}", hours, minutes, seconds));
    }

    if parts.is_empty() {
        Ok(serde_json::Value::String("00:00:00".to_string()))
    } else {
        Ok(serde_json::Value::String(parts.join(" ")))
    }
}

pub fn decode_binary_array(
    raw: &[u8],
    element_type: &Type,
    registry: Option<&CustomTypeRegistry>,
) -> Result<serde_json::Value, String> {
    if raw.len() < 12 {
        return Err("Invalid array header".to_string());
    }
    let ndim = u32::from_be_bytes(raw[0..4].try_into().unwrap()) as usize;
    let _flags = u32::from_be_bytes(raw[4..8].try_into().unwrap());
    let _element_oid = u32::from_be_bytes(raw[8..12].try_into().unwrap());

    if ndim == 0 {
        return Ok(serde_json::Value::Array(vec![]));
    }

    let header_size = 12 + ndim * 8;
    if raw.len() < header_size {
        return Err("Invalid array dimensions".to_string());
    }

    let mut cursor = header_size;
    let mut elements = vec![];

    while cursor < raw.len() {
        if cursor + 4 > raw.len() {
            break;
        }
        let len_i32 = i32::from_be_bytes(raw[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;

        if len_i32 == -1 {
            elements.push(serde_json::Value::Null);
        } else {
            let el_len = len_i32 as usize;
            if cursor + el_len > raw.len() {
                return Err("Array element length overflow".to_string());
            }
            let el_data = &raw[cursor..cursor + el_len];
            cursor += el_len;

            let val = decode_raw_value(el_data, element_type, registry);
            elements.push(val);
        }
    }

    Ok(serde_json::Value::Array(elements))
}

pub fn decode_raw_value(
    raw: &[u8],
    ty: &Type,
    registry: Option<&CustomTypeRegistry>,
) -> serde_json::Value {
    match *ty {
        Type::INTERVAL => {
            if let Ok(val) = decode_interval(raw) {
                val
            } else {
                serde_json::Value::Null
            }
        }
        Type::BYTEA => {
            let mut s = String::with_capacity(raw.len() * 2 + 2);
            s.push_str("\\x");
            for &b in raw {
                s.push(std::char::from_digit((b >> 4) as u32, 16).unwrap());
                s.push(std::char::from_digit((b & 0xf) as u32, 16).unwrap());
            }
            serde_json::Value::String(s)
        }
        Type::INET | Type::CIDR => {
            if raw.len() >= 4 {
                let family = raw[0];
                let len = raw[3] as usize;
                if raw.len() >= 4 + len {
                    let addr = &raw[4..4 + len];
                    if family == 2 && len == 4 {
                        let ip = std::net::Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]);
                        return serde_json::Value::String(ip.to_string());
                    } else if family == 3 && len == 16 {
                        let ip = std::net::Ipv6Addr::from(<[u8; 16]>::try_from(addr).unwrap());
                        return serde_json::Value::String(ip.to_string());
                    }
                }
            }
            serde_json::Value::String(String::from_utf8_lossy(raw).into_owned())
        }
        Type::MACADDR => {
            if raw.len() == 6 {
                let s = format!(
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    raw[0], raw[1], raw[2], raw[3], raw[4], raw[5]
                );
                serde_json::Value::String(s)
            } else {
                serde_json::Value::String(String::from_utf8_lossy(raw).into_owned())
            }
        }
        Type::OID => {
            if raw.len() == 4 {
                let val = u32::from_be_bytes(raw.try_into().unwrap());
                serde_json::Value::Number(val.into())
            } else {
                serde_json::Value::Null
            }
        }
        Type::XML => serde_json::Value::String(String::from_utf8_lossy(raw).into_owned()),
        Type::MONEY => {
            if raw.len() == 8 {
                let val = i64::from_be_bytes(raw.try_into().unwrap());
                serde_json::Value::String(format!("{:.2}", (val as f64) / 100.0))
            } else {
                serde_json::Value::Null
            }
        }
        Type::BIT | Type::VARBIT => {
            if raw.len() >= 4 {
                let bit_len = u32::from_be_bytes(raw[0..4].try_into().unwrap()) as usize;
                let mut s = String::with_capacity(bit_len);
                for i in 0..bit_len {
                    let byte_idx = 4 + (i / 8);
                    let bit_idx = 7 - (i % 8);
                    if byte_idx < raw.len() {
                        let bit = (raw[byte_idx] >> bit_idx) & 1;
                        s.push(if bit == 1 { '1' } else { '0' });
                    }
                }
                serde_json::Value::String(s)
            } else {
                serde_json::Value::String(String::from_utf8_lossy(raw).into_owned())
            }
        }
        Type::BOOL => {
            if !raw.is_empty() {
                serde_json::Value::Bool(raw[0] != 0)
            } else {
                serde_json::Value::Null
            }
        }
        Type::INT2 => {
            if raw.len() == 2 {
                let val = i16::from_be_bytes(raw.try_into().unwrap());
                serde_json::Value::Number(val.into())
            } else {
                serde_json::Value::Null
            }
        }
        Type::INT4 => {
            if raw.len() == 4 {
                let val = i32::from_be_bytes(raw.try_into().unwrap());
                serde_json::Value::Number(val.into())
            } else {
                serde_json::Value::Null
            }
        }
        Type::INT8 => {
            if raw.len() == 8 {
                let val = i64::from_be_bytes(raw.try_into().unwrap());
                serde_json::Value::Number(val.into())
            } else {
                serde_json::Value::Null
            }
        }
        Type::FLOAT4 => {
            if raw.len() == 4 {
                let val = f32::from_be_bytes(raw.try_into().unwrap());
                if let Some(num) = serde_json::Number::from_f64(val as f64) {
                    serde_json::Value::Number(num)
                } else {
                    serde_json::Value::Null
                }
            } else {
                serde_json::Value::Null
            }
        }
        Type::FLOAT8 => {
            if raw.len() == 8 {
                let val = f64::from_be_bytes(raw.try_into().unwrap());
                if let Some(num) = serde_json::Number::from_f64(val) {
                    serde_json::Value::Number(num)
                } else {
                    serde_json::Value::Null
                }
            } else {
                serde_json::Value::Null
            }
        }
        Type::VARCHAR | Type::TEXT | Type::BPCHAR | Type::NAME => {
            let s = String::from_utf8_lossy(raw).into_owned();
            serde_json::Value::String(s)
        }
        Type::JSON => {
            if let Ok(val) = serde_json::from_slice(raw) {
                val
            } else {
                serde_json::Value::String(String::from_utf8_lossy(raw).into_owned())
            }
        }
        Type::JSONB => {
            if !raw.is_empty() {
                let data = &raw[1..];
                if let Ok(val) = serde_json::from_slice(data) {
                    val
                } else {
                    serde_json::Value::String(String::from_utf8_lossy(data).into_owned())
                }
            } else {
                serde_json::Value::Null
            }
        }
        _ => {
            if ty.name() == "hstore" {
                if let Ok(val) = decode_hstore(raw) {
                    return val;
                }
            }

            if let tokio_postgres::types::Kind::Enum(_) = ty.kind() {
                let s = String::from_utf8_lossy(raw).into_owned();
                return serde_json::Value::String(s);
            }

            if let Some(reg) = registry {
                if let Some(info) = reg.get_by_oid(ty.oid()) {
                    if info.typtype == 'e' {
                        let s = String::from_utf8_lossy(raw).into_owned();
                        return serde_json::Value::String(s);
                    }
                }
            }

            if let Ok(s) = std::str::from_utf8(raw) {
                serde_json::Value::String(s.to_string())
            } else {
                serde_json::Value::String(format!("<type: {}>", ty.name()))
            }
        }
    }
}

pub fn pg_value_to_json(
    row: &tokio_postgres::Row,
    index: usize,
    registry: Option<&CustomTypeRegistry>,
) -> serde_json::Value {
    let col = &row.columns()[index];
    let ty = col.type_();

    if let tokio_postgres::types::Kind::Array(elem_ty) = ty.kind() {
        if let Ok(Some(raw_val)) = row.try_get::<_, Option<RawValue>>(index) {
            if let Ok(arr) = decode_binary_array(raw_val.0, elem_ty, registry) {
                return arr;
            }
        }
    }

    match *ty {
        Type::INTERVAL => match row.try_get::<_, Option<RawValue>>(index) {
            Ok(Some(raw)) => decode_interval(raw.0).unwrap_or(serde_json::Value::Null),
            _ => serde_json::Value::Null,
        },
        Type::BYTEA => match row.try_get::<_, Option<Vec<u8>>>(index) {
            Ok(Some(bytes)) => {
                let mut s = String::with_capacity(bytes.len() * 2 + 2);
                s.push_str("\\x");
                for &b in &bytes {
                    s.push(std::char::from_digit((b >> 4) as u32, 16).unwrap());
                    s.push(std::char::from_digit((b & 0xf) as u32, 16).unwrap());
                }
                serde_json::Value::String(s)
            }
            _ => serde_json::Value::Null,
        },
        Type::INET | Type::CIDR => match row.try_get::<_, Option<std::net::IpAddr>>(index) {
            Ok(Some(ip)) => serde_json::Value::String(ip.to_string()),
            _ => serde_json::Value::Null,
        },
        Type::MACADDR => match row.try_get::<_, Option<RawValue>>(index) {
            Ok(Some(raw_val)) => {
                if raw_val.0.len() == 6 {
                    let s = format!(
                        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                        raw_val.0[0],
                        raw_val.0[1],
                        raw_val.0[2],
                        raw_val.0[3],
                        raw_val.0[4],
                        raw_val.0[5]
                    );
                    serde_json::Value::String(s)
                } else {
                    serde_json::Value::String(String::from_utf8_lossy(raw_val.0).into_owned())
                }
            }
            _ => serde_json::Value::Null,
        },
        Type::OID => match row.try_get::<_, Option<u32>>(index) {
            Ok(Some(oid)) => serde_json::Value::Number(oid.into()),
            _ => serde_json::Value::Null,
        },
        Type::XML => match row.try_get::<_, Option<String>>(index) {
            Ok(Some(val)) => serde_json::Value::String(val),
            _ => serde_json::Value::Null,
        },
        Type::MONEY => match row.try_get::<_, Option<i64>>(index) {
            Ok(Some(val)) => serde_json::Value::String(format!("{:.2}", (val as f64) / 100.0)),
            _ => serde_json::Value::Null,
        },
        Type::BIT | Type::VARBIT => match row.try_get::<_, Option<RawValue>>(index) {
            Ok(Some(raw_val)) => {
                if raw_val.0.len() >= 4 {
                    let bit_len = u32::from_be_bytes(raw_val.0[0..4].try_into().unwrap()) as usize;
                    let mut s = String::with_capacity(bit_len);
                    for i in 0..bit_len {
                        let byte_idx = 4 + (i / 8);
                        let bit_idx = 7 - (i % 8);
                        if byte_idx < raw_val.0.len() {
                            let bit = (raw_val.0[byte_idx] >> bit_idx) & 1;
                            s.push(if bit == 1 { '1' } else { '0' });
                        }
                    }
                    serde_json::Value::String(s)
                } else {
                    serde_json::Value::String(String::from_utf8_lossy(raw_val.0).into_owned())
                }
            }
            _ => serde_json::Value::Null,
        },
        Type::BOOL => match row.try_get::<_, Option<bool>>(index) {
            Ok(Some(val)) => serde_json::Value::Bool(val),
            _ => serde_json::Value::Null,
        },
        Type::INT2 => match row.try_get::<_, Option<i16>>(index) {
            Ok(Some(val)) => serde_json::Value::Number(val.into()),
            _ => serde_json::Value::Null,
        },
        Type::INT4 => match row.try_get::<_, Option<i32>>(index) {
            Ok(Some(val)) => serde_json::Value::Number(val.into()),
            _ => serde_json::Value::Null,
        },
        Type::INT8 => match row.try_get::<_, Option<i64>>(index) {
            Ok(Some(val)) => serde_json::Value::Number(val.into()),
            _ => serde_json::Value::Null,
        },
        Type::FLOAT4 => match row.try_get::<_, Option<f32>>(index) {
            Ok(Some(val)) => {
                if let Some(n) = serde_json::Number::from_f64(val as f64) {
                    serde_json::Value::Number(n)
                } else {
                    serde_json::Value::Null
                }
            }
            _ => serde_json::Value::Null,
        },
        Type::FLOAT8 => match row.try_get::<_, Option<f64>>(index) {
            Ok(Some(val)) => {
                if let Some(n) = serde_json::Number::from_f64(val) {
                    serde_json::Value::Number(n)
                } else {
                    serde_json::Value::Null
                }
            }
            _ => serde_json::Value::Null,
        },
        Type::VARCHAR | Type::TEXT | Type::BPCHAR | Type::NAME => {
            match row.try_get::<_, Option<String>>(index) {
                Ok(Some(val)) => serde_json::Value::String(val),
                _ => serde_json::Value::Null,
            }
        }
        Type::JSON | Type::JSONB => match row.try_get::<_, Option<serde_json::Value>>(index) {
            Ok(Some(val)) => val,
            _ => serde_json::Value::Null,
        },
        Type::TIMESTAMP | Type::TIMESTAMPTZ => {
            if let Ok(val_opt) = row.try_get::<_, Option<chrono::NaiveDateTime>>(index) {
                match val_opt {
                    Some(val) => serde_json::Value::String(val.to_string()),
                    None => serde_json::Value::Null,
                }
            } else if let Ok(val_opt) =
                row.try_get::<_, Option<chrono::DateTime<chrono::Utc>>>(index)
            {
                match val_opt {
                    Some(val) => serde_json::Value::String(val.to_string()),
                    None => serde_json::Value::Null,
                }
            } else {
                serde_json::Value::String("<timestamp>".to_string())
            }
        }
        Type::DATE => {
            if let Ok(val_opt) = row.try_get::<_, Option<chrono::NaiveDate>>(index) {
                match val_opt {
                    Some(val) => serde_json::Value::String(val.to_string()),
                    None => serde_json::Value::Null,
                }
            } else {
                serde_json::Value::String("<date>".to_string())
            }
        }
        Type::UUID => match row.try_get::<_, Option<uuid::Uuid>>(index) {
            Ok(Some(val)) => serde_json::Value::String(val.to_string()),
            _ => serde_json::Value::Null,
        },
        Type::NUMERIC => match row.try_get::<_, Option<rust_decimal::Decimal>>(index) {
            Ok(Some(val)) => serde_json::to_value(val).unwrap_or(serde_json::Value::Null),
            _ => serde_json::Value::Null,
        },
        _ => {
            if ty.name() == "hstore" {
                match row.try_get::<_, Option<RawValue>>(index) {
                    Ok(Some(raw)) => {
                        return decode_hstore(raw.0).unwrap_or(serde_json::Value::Null)
                    }
                    _ => return serde_json::Value::Null,
                }
            }

            if let tokio_postgres::types::Kind::Enum(_) = ty.kind() {
                if let Ok(Some(raw_val)) = row.try_get::<_, Option<RawValue>>(index) {
                    if let Ok(s) = std::str::from_utf8(raw_val.0) {
                        return serde_json::Value::String(s.to_string());
                    }
                }
            }

            if let Some(reg) = registry {
                if let Some(info) = reg.get_by_oid(ty.oid()) {
                    if info.typtype == 'e' {
                        if let Ok(Some(raw_val)) = row.try_get::<_, Option<RawValue>>(index) {
                            if let Ok(s) = std::str::from_utf8(raw_val.0) {
                                return serde_json::Value::String(s.to_string());
                            }
                        }
                    }
                }
            }

            if let Ok(Some(val)) = row.try_get::<_, Option<String>>(index) {
                serde_json::Value::String(val)
            } else {
                if let Ok(None) = row.try_get::<_, Option<String>>(index) {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(format!("<type: {}>", ty.name()))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_type_registry_resolution() {
        let mut reg = CustomTypeRegistry::new();
        reg.insert(CustomTypeInfo {
            oid: 12000,
            name: "mood".to_string(),
            schema: "public".to_string(),
            typtype: 'e',
            typcategory: 'E',
            typelem: 0,
            typbasetype: 0,
            base_type_name: None,
            full_type_name: "mood".to_string(),
            description: None,
        });
        reg.insert(CustomTypeInfo {
            oid: 12001,
            name: "mood".to_string(),
            schema: "custom_schema".to_string(),
            typtype: 'e',
            typcategory: 'E',
            typelem: 0,
            typbasetype: 0,
            base_type_name: None,
            full_type_name: "mood".to_string(),
            description: None,
        });

        // Resolve OID
        assert_eq!(reg.get_by_oid(12000).unwrap().schema, "public".to_string());

        // Resolve by qualified name
        assert_eq!(
            reg.resolve_by_name("custom_schema.mood", &[]).unwrap().oid,
            12001
        );

        // Resolve unqualified respecting search path
        let search_path = vec!["custom_schema".to_string(), "public".to_string()];
        assert_eq!(
            reg.resolve_by_name("mood", &search_path).unwrap().oid,
            12001
        );

        let search_path_other = vec!["public".to_string(), "custom_schema".to_string()];
        assert_eq!(
            reg.resolve_by_name("mood", &search_path_other).unwrap().oid,
            12000
        );
    }

    #[test]
    fn test_decode_binary_array() {
        // Mock a 1D bool array binary payload:
        // ndims: 1 (4 bytes: 0 0 0 1)
        // flags: 0 (4 bytes: 0 0 0 0)
        // elem_oid: 16 (4 bytes: 0 0 0 16)
        // dim_len: 3 (4 bytes: 0 0 0 3)
        // dim_lbound: 1 (4 bytes: 0 0 0 1)
        // elem1_len: 1 (4 bytes: 0 0 0 1)
        // elem1_val: 1 (1 byte: 1)
        // elem2_len: 1 (4 bytes: 0 0 0 1)
        // elem2_val: 0 (1 byte: 0)
        // elem3_len: -1 (4 bytes: 255 255 255 255)
        let raw = vec![
            0, 0, 0, 1, // ndims
            0, 0, 0, 0, // flags
            0, 0, 0, 16, // elem_oid (Type::BOOL is 16)
            0, 0, 0, 3, // dim_len
            0, 0, 0, 1, // lbound
            0, 0, 0, 1, // elem1 length
            1, // elem1 value (true)
            0, 0, 0, 1, // elem2 length
            0, // elem2 value (false)
            255, 255, 255, 255, // elem3 length (-1, NULL)
        ];

        let result = decode_binary_array(&raw, &Type::BOOL, None).unwrap();
        assert_eq!(
            result,
            serde_json::Value::Array(vec![
                serde_json::Value::Bool(true),
                serde_json::Value::Bool(false),
                serde_json::Value::Null
            ])
        );
    }

    #[test]
    fn test_decode_interval() {
        let raw = vec![
            0, 0, 0, 0, 0xD6, 0x93, 0xA4, 0x00, // 1 hour (micros)
            0, 0, 0, 5, // 5 days
            0, 0, 0, 14, // 14 months (1 year, 2 months)
        ];
        let val = decode_interval(&raw).unwrap();
        assert_eq!(
            val,
            serde_json::Value::String("1 year 2 months 5 days 01:00:00.000".to_string())
        );
    }

    #[test]
    fn test_decode_hstore() {
        let raw = vec![
            0, 0, 0, 2, // 2 pairs
            0, 0, 0, 1, 97, // "a"
            0, 0, 0, 1, 98, // "b"
            0, 0, 0, 1, 120, // "x"
            255, 255, 255, 255, // null
        ];
        let val = decode_hstore(&raw).unwrap();
        let mut map = serde_json::Map::new();
        map.insert("a".to_string(), serde_json::Value::String("b".to_string()));
        map.insert("x".to_string(), serde_json::Value::Null);
        assert_eq!(val, serde_json::Value::Object(map));
    }
}
