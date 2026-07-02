use std::collections::HashMap;
use tokio_postgres::types::Type;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomTypeInfo {
    pub oid: u32,
    pub name: String,
    pub schema: String,
    pub typtype: char, // 'e' = enum, 'c' = composite, 'd' = domain, 'b' = base
}

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
            if let Ok(arr) = decode_binary_array(raw_val.0, &elem_ty, registry) {
                return arr;
            }
        }
    }

    match *ty {
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
        });
        reg.insert(CustomTypeInfo {
            oid: 12001,
            name: "mood".to_string(),
            schema: "custom_schema".to_string(),
            typtype: 'e',
        });

        // Resolve OID
        assert_eq!(
            reg.get_by_oid(12000).unwrap().schema,
            "public".to_string()
        );

        // Resolve by qualified name
        assert_eq!(
            reg.resolve_by_name("custom_schema.mood", &[])
                .unwrap()
                .oid,
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
            reg.resolve_by_name("mood", &search_path_other)
                .unwrap()
                .oid,
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
}
