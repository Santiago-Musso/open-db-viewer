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

pub fn pg_value_to_json(row: &tokio_postgres::Row, index: usize) -> serde_json::Value {
    let col = &row.columns()[index];
    let ty = col.type_();

    match *ty {
        tokio_postgres::types::Type::BOOL => match row.try_get::<_, Option<bool>>(index) {
            Ok(Some(val)) => serde_json::Value::Bool(val),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::INT2 => match row.try_get::<_, Option<i16>>(index) {
            Ok(Some(val)) => serde_json::Value::Number(val.into()),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::INT4 => match row.try_get::<_, Option<i32>>(index) {
            Ok(Some(val)) => serde_json::Value::Number(val.into()),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::INT8 => match row.try_get::<_, Option<i64>>(index) {
            Ok(Some(val)) => serde_json::Value::Number(val.into()),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::FLOAT4 => match row.try_get::<_, Option<f32>>(index) {
            Ok(Some(val)) => {
                if let Some(n) = serde_json::Number::from_f64(val as f64) {
                    serde_json::Value::Number(n)
                } else {
                    serde_json::Value::Null
                }
            }
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::FLOAT8 => match row.try_get::<_, Option<f64>>(index) {
            Ok(Some(val)) => {
                if let Some(n) = serde_json::Number::from_f64(val) {
                    serde_json::Value::Number(n)
                } else {
                    serde_json::Value::Null
                }
            }
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::VARCHAR
        | tokio_postgres::types::Type::TEXT
        | tokio_postgres::types::Type::BPCHAR
        | tokio_postgres::types::Type::NAME => match row.try_get::<_, Option<String>>(index) {
            Ok(Some(val)) => serde_json::Value::String(val),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::JSON | tokio_postgres::types::Type::JSONB => {
            match row.try_get::<_, Option<serde_json::Value>>(index) {
                Ok(Some(val)) => val,
                _ => serde_json::Value::Null,
            }
        }
        tokio_postgres::types::Type::TIMESTAMP | tokio_postgres::types::Type::TIMESTAMPTZ => {
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
        tokio_postgres::types::Type::DATE => {
            if let Ok(val_opt) = row.try_get::<_, Option<chrono::NaiveDate>>(index) {
                match val_opt {
                    Some(val) => serde_json::Value::String(val.to_string()),
                    None => serde_json::Value::Null,
                }
            } else {
                serde_json::Value::String("<date>".to_string())
            }
        }
        tokio_postgres::types::Type::UUID => match row.try_get::<_, Option<uuid::Uuid>>(index) {
            Ok(Some(val)) => serde_json::Value::String(val.to_string()),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::NUMERIC => {
            match row.try_get::<_, Option<rust_decimal::Decimal>>(index) {
                Ok(Some(val)) => serde_json::to_value(val).unwrap_or(serde_json::Value::Null),
                _ => serde_json::Value::Null,
            }
        }
        _ => {
            if let tokio_postgres::types::Kind::Enum(_) = ty.kind() {
                if let Ok(Some(raw_val)) = row.try_get::<_, Option<RawValue>>(index) {
                    if let Ok(s) = std::str::from_utf8(raw_val.0) {
                        return serde_json::Value::String(s.to_string());
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
