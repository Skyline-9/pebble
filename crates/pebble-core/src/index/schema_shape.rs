//! Allocation-bounded reads of untrusted `sqlite_schema` values.

use rusqlite::{Connection, params};

use super::IndexError;

const MAX_SCHEMA_OBJECTS: i64 = 64;
const MAX_SCHEMA_IDENTIFIER_BYTES: i64 = 255;
const MAX_SCHEMA_SQL_BYTES: i64 = 65_536;

pub(super) type SchemaRow = (String, String, String, Option<String>);

pub(super) fn rows(connection: &Connection) -> Result<Vec<SchemaRow>, IndexError> {
    let count = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if !(0..=MAX_SCHEMA_OBJECTS).contains(&count) {
        return Err(IndexError::rebuild(
            "SQLite graph has too many schema objects",
        ));
    }
    reject_oversized_values(connection)?;
    let mut statement = connection.prepare(
        "SELECT
            substr(CAST(type AS BLOB), 1, ?1),
            substr(CAST(name AS BLOB), 1, ?1),
            substr(CAST(tbl_name AS BLOB), 1, ?1),
            CASE WHEN sql IS NULL THEN NULL
                 ELSE substr(CAST(sql AS BLOB), 1, ?2)
            END
         FROM sqlite_schema
         WHERE name NOT LIKE 'sqlite_%'
         ORDER BY type, name
         LIMIT ?3",
    )?;
    Ok(statement
        .query_map(
            params![
                MAX_SCHEMA_IDENTIFIER_BYTES,
                MAX_SCHEMA_SQL_BYTES,
                MAX_SCHEMA_OBJECTS + 1
            ],
            |row| {
                Ok((
                    bounded_text(row.get::<_, Vec<u8>>(0)?)?,
                    bounded_text(row.get::<_, Vec<u8>>(1)?)?,
                    bounded_text(row.get::<_, Vec<u8>>(2)?)?,
                    row.get::<_, Option<Vec<u8>>>(3)?
                        .map(bounded_text)
                        .transpose()?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(super) fn expected() -> Result<Vec<SchemaRow>, IndexError> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(super::schema::SCHEMA)?;
    rows(&connection)
}

fn reject_oversized_values(connection: &Connection) -> Result<(), IndexError> {
    let oversized = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_schema
         WHERE name NOT LIKE 'sqlite_%' AND (
            typeof(type) != 'text'
            OR length(CAST(type AS BLOB)) > ?1
            OR typeof(name) != 'text'
            OR length(CAST(name AS BLOB)) > ?1
            OR typeof(tbl_name) != 'text'
            OR length(CAST(tbl_name AS BLOB)) > ?1
            OR (sql IS NOT NULL AND (
                typeof(sql) != 'text' OR length(CAST(sql AS BLOB)) > ?2
            ))
         )",
        params![MAX_SCHEMA_IDENTIFIER_BYTES, MAX_SCHEMA_SQL_BYTES],
        |row| row.get::<_, i64>(0),
    )?;
    if oversized != 0 {
        return Err(IndexError::rebuild(
            "SQLite schema definition exceeds its size limit",
        ));
    }
    Ok(())
}

fn bounded_text(bytes: Vec<u8>) -> Result<String, rusqlite::Error> {
    String::from_utf8(bytes).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(error))
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    #[test]
    fn rejects_huge_schema_sql_before_materializing_rows() -> Result<(), Box<dyn std::error::Error>>
    {
        let connection = Connection::open_in_memory()?;
        let huge_comment = "x".repeat(4 * 1_024 * 1_024);
        connection.execute_batch(&format!(
            "CREATE TABLE adversarial(value TEXT /*{huge_comment}*/);"
        ))?;

        let error = super::rows(&connection)
            .err()
            .ok_or("huge schema was accepted")?;
        assert!(error.to_string().contains("exceeds its size limit"));
        Ok(())
    }
}
