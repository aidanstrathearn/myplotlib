use crate::Points;
use eframe::egui;
use std::path::Path;

const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_DATA_ROWS: usize = 250_000;

#[derive(Debug)]
pub(crate) struct ImportedSeries {
    pub(crate) id: u64,
    pub(crate) label: String,
    pub(crate) source_name: String,
    pub(crate) points: Points,
}

pub(crate) fn import_dropped_file(file: &egui::DroppedFile) -> Result<Vec<ImportedSeries>, String> {
    let source_name = dropped_file_name(file);
    let bytes = dropped_file_bytes(file, &source_name)?;
    parse_data(&source_name, &file.mime, &bytes).map(|series| vec![series])
}

fn dropped_file_name(file: &egui::DroppedFile) -> String {
    if !file.name.trim().is_empty() {
        return file.name.clone();
    }

    file.path
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dropped file".to_owned())
}

fn dropped_file_bytes(file: &egui::DroppedFile, source_name: &str) -> Result<Vec<u8>, String> {
    if let Some(bytes) = &file.bytes {
        check_size(source_name, bytes.len())?;
        return Ok(bytes.to_vec());
    }

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &file.path {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("{source_name}: could not inspect the file: {error}"))?;
        let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        check_size(source_name, size)?;

        let bytes = std::fs::read(path)
            .map_err(|error| format!("{source_name}: could not read the file: {error}"))?;
        check_size(source_name, bytes.len())?;
        return Ok(bytes);
    }

    Err(format!(
        "{source_name}: the dropped file has no readable contents"
    ))
}

fn check_size(source_name: &str, size: usize) -> Result<(), String> {
    if size > MAX_FILE_BYTES {
        return Err(format!(
            "{source_name}: the file is larger than the {} MiB import limit",
            MAX_FILE_BYTES / (1024 * 1024)
        ));
    }
    Ok(())
}

fn parse_data(source_name: &str, mime: &str, bytes: &[u8]) -> Result<ImportedSeries, String> {
    check_size(source_name, bytes.len())?;
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let delimiter = delimiter_for(source_name, mime, bytes);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    let mut points = Vec::new();
    let mut label = default_label(source_name);
    let mut first_record = true;
    let mut had_header = false;

    for result in reader.records() {
        let record = result.map_err(|error| csv_error(source_name, error))?;
        if record.is_empty() {
            continue;
        }
        let line = record.position().map_or(1, csv::Position::line);
        if record.len() != 2 {
            return Err(format!(
                "{source_name}: row {line} contains {} columns; expected exactly 2",
                record.len()
            ));
        }

        if first_record {
            first_record = false;
            let x = classify_number(&record[0]);
            let y = classify_number(&record[1]);
            if matches!((&x, &y), (NumberField::Text, NumberField::Text)) {
                had_header = true;
                if !record[1].is_empty() && !record[1].eq_ignore_ascii_case("y") {
                    label = record[1].to_owned();
                }
                continue;
            }

            points.push([
                require_number(source_name, line, 1, &record[0], x)?,
                require_number(source_name, line, 2, &record[1], y)?,
            ]);
            continue;
        }

        points.push([
            parse_number(source_name, line, 1, &record[0])?,
            parse_number(source_name, line, 2, &record[1])?,
        ]);
        if points.len() > MAX_DATA_ROWS {
            return Err(format!(
                "{source_name}: the file contains more than the {MAX_DATA_ROWS} row import limit"
            ));
        }
    }

    if first_record {
        return Err(format!("{source_name}: the file contains no data"));
    }
    if points.is_empty() {
        let detail = if had_header { " after its header" } else { "" };
        return Err(format!(
            "{source_name}: the file contains no data rows{detail}"
        ));
    }

    Ok(ImportedSeries {
        id: 0,
        label,
        source_name: source_name.to_owned(),
        points,
    })
}

fn delimiter_for(source_name: &str, mime: &str, bytes: &[u8]) -> u8 {
    let extension = Path::new(source_name)
        .extension()
        .and_then(|extension| extension.to_str());
    if extension.is_some_and(|extension| extension.eq_ignore_ascii_case("tsv"))
        || mime.eq_ignore_ascii_case("text/tab-separated-values")
    {
        return b'\t';
    }
    if extension.is_some_and(|extension| extension.eq_ignore_ascii_case("csv")) {
        return b',';
    }

    let first_line = bytes
        .split(|byte| *byte == b'\n' || *byte == b'\r')
        .find(|line| !line.iter().all(u8::is_ascii_whitespace))
        .unwrap_or_default();
    if first_line.contains(&b'\t') && !first_line.contains(&b',') {
        b'\t'
    } else {
        b','
    }
}

fn default_label(source_name: &str) -> String {
    Path::new(source_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or(source_name)
        .to_owned()
}

enum NumberField {
    Number(f64),
    NonFinite,
    Text,
}

fn classify_number(value: &str) -> NumberField {
    match value.parse::<f64>() {
        Ok(value) if value.is_finite() => NumberField::Number(value),
        Ok(_) => NumberField::NonFinite,
        Err(_) => NumberField::Text,
    }
}

fn parse_number(source_name: &str, line: u64, column: usize, value: &str) -> Result<f64, String> {
    require_number(source_name, line, column, value, classify_number(value))
}

fn require_number(
    source_name: &str,
    line: u64,
    column: usize,
    source: &str,
    value: NumberField,
) -> Result<f64, String> {
    match value {
        NumberField::Number(value) => Ok(value),
        NumberField::NonFinite => Err(format!(
            "{source_name}: row {line}, column {column} is not finite: {source:?}"
        )),
        NumberField::Text => Err(format!(
            "{source_name}: row {line}, column {column} is not a number: {source:?}"
        )),
    }
}

fn csv_error(source_name: &str, error: csv::Error) -> String {
    if let Some(position) = error.position() {
        format!(
            "{source_name}: could not parse row {}: {error}",
            position.line()
        )
    } else {
        format!("{source_name}: could not parse the file: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn parses_headerless_csv_and_uses_the_filename_as_its_label() {
        let series = parse_data("measurements.csv", "text/csv", b"0,1\n2.5,3.5\n").unwrap();

        assert_eq!(series.label, "measurements");
        assert_eq!(series.source_name, "measurements.csv");
        assert_eq!(series.points, vec![[0.0, 1.0], [2.5, 3.5]]);
    }

    #[test]
    fn parses_quoted_tsv_headers_and_a_utf8_bom() {
        let series = parse_data(
            "measurements.tsv",
            "text/tab-separated-values",
            b"\xef\xbb\xbf\"time\"\t\"voltage\"\n0\t1\n2\t3\n",
        )
        .unwrap();

        assert_eq!(series.label, "voltage");
        assert_eq!(series.points, vec![[0.0, 1.0], [2.0, 3.0]]);
    }

    #[test]
    fn reports_bad_shape_numbers_and_non_finite_values() {
        let error = parse_data("bad.csv", "text/csv", b"x,y\n0,1,2\n").unwrap_err();
        assert!(error.contains("row 2 contains 3 columns"), "{error}");

        let error = parse_data("bad.csv", "text/csv", b"1,nope\n2,3\n").unwrap_err();
        assert!(error.contains("row 1, column 2 is not a number"), "{error}");

        let error = parse_data("bad.csv", "text/csv", b"x,y\n0,NaN\n").unwrap_err();
        assert!(error.contains("row 2, column 2 is not finite"), "{error}");
    }

    #[test]
    fn imports_web_style_bytes() {
        let file = egui::DroppedFile {
            name: "web.csv".to_owned(),
            mime: "text/csv".to_owned(),
            bytes: Some(Arc::from(&b"x,y\n0,1\n"[..])),
            ..Default::default()
        };

        let imported = import_dropped_file(&file).unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].points, vec![[0.0, 1.0]]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn imports_native_paths() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "myplotlib-data-import-{}-{}.csv",
            std::process::id(),
            unique
        ));
        std::fs::write(&path, b"x,y\n0,1\n").unwrap();
        let file = egui::DroppedFile {
            path: Some(path.clone()),
            ..Default::default()
        };

        let result = import_dropped_file(&file);
        std::fs::remove_file(path).unwrap();

        assert_eq!(result.unwrap()[0].points, vec![[0.0, 1.0]]);
    }
}
