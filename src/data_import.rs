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
    pub(crate) y_uncertainties: Option<Vec<f64>>,
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
    let mut y_uncertainties = None;
    let mut label = default_label(source_name);
    let mut columns = None;
    let mut had_header = false;

    for result in reader.records() {
        let record = result.map_err(|error| csv_error(source_name, error))?;
        if record.is_empty() {
            continue;
        }
        let line = record.position().map_or(1, csv::Position::line);
        if let Some(expected) = columns {
            if record.len() != expected {
                return Err(format!(
                    "{source_name}: row {line} contains {} columns; expected exactly {expected}",
                    record.len()
                ));
            }
        } else {
            if !matches!(record.len(), 2 | 3) {
                return Err(format!(
                    "{source_name}: row {line} contains {} columns; expected 2 or 3",
                    record.len()
                ));
            }
            columns = Some(record.len());
            y_uncertainties = (record.len() == 3).then(Vec::new);
            if record
                .iter()
                .all(|field| matches!(classify_number(field), NumberField::Text))
            {
                had_header = true;
                if !record[1].is_empty() && !record[1].eq_ignore_ascii_case("y") {
                    label = record[1].to_owned();
                }
                continue;
            }
        }

        let x = parse_number(source_name, line, 1, &record[0])?;
        let y = parse_number(source_name, line, 2, &record[1])?;
        if let Some(uncertainties) = &mut y_uncertainties {
            let uncertainty = parse_number(source_name, line, 3, &record[2])?;
            if uncertainty < 0.0 {
                return Err(format!(
                    "{source_name}: row {line}, column 3 must be a nonnegative uncertainty: {:?}",
                    &record[2]
                ));
            }
            if !(y - uncertainty).is_finite() || !(y + uncertainty).is_finite() {
                return Err(format!(
                    "{source_name}: row {line}, column 3 produces non-finite error bar endpoints"
                ));
            }
            uncertainties.push(uncertainty);
        }
        points.push([x, y]);
        if points.len() > MAX_DATA_ROWS {
            return Err(format!(
                "{source_name}: the file contains more than the {MAX_DATA_ROWS} row import limit"
            ));
        }
    }

    if columns.is_none() {
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
        y_uncertainties,
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
        assert_eq!(series.y_uncertainties, None);
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
        assert_eq!(series.y_uncertainties, None);
    }

    #[test]
    fn parses_rabi_headers_and_scientific_notation_with_uncertainties() {
        let series = parse_data(
            "rabi_data_raw.csv",
            "text/csv",
            b"yellow_pulse_time,ratio,u_ratio\n0,0.9724572703,0.002945120449\n5e-05,0.9533036853,0.002885116733\n",
        )
        .unwrap();

        assert_eq!(series.label, "ratio");
        assert_eq!(
            series.points,
            vec![[0.0, 0.9724572703], [5e-5, 0.9533036853]]
        );
        assert_eq!(
            series.y_uncertainties,
            Some(vec![0.002945120449, 0.002885116733])
        );
    }

    #[test]
    fn parses_headerless_uncertainties_including_zero() {
        let series = parse_data("values.csv", "", b"0,1,0\n2,3,4e-2\n").unwrap();

        assert_eq!(series.label, "values");
        assert_eq!(series.points, vec![[0.0, 1.0], [2.0, 3.0]]);
        assert_eq!(series.y_uncertainties, Some(vec![0.0, 0.04]));
    }

    #[test]
    fn parses_uncertainties_with_quoted_tsv_headers_and_a_bom() {
        let series = parse_data(
            "values.tsv",
            "text/tab-separated-values",
            b"\xef\xbb\xbf\"time\"\t\"ratio\"\t\"u_ratio\"\n0\t1\t0.1\n",
        )
        .unwrap();

        assert_eq!(series.label, "ratio");
        assert_eq!(series.y_uncertainties, Some(vec![0.1]));
    }

    #[test]
    fn rejects_invalid_uncertainties_with_their_row_and_column() {
        for (value, message) in [
            ("-0.1", "must be a nonnegative uncertainty"),
            ("NaN", "is not finite"),
            ("inf", "is not finite"),
            ("nope", "is not a number"),
            ("", "is not a number"),
        ] {
            let csv = format!("x,y,u_y\n0,1,{value}\n");
            let error = parse_data("bad.csv", "text/csv", csv.as_bytes()).unwrap_err();
            assert!(error.contains("row 2, column 3"), "{error}");
            assert!(error.contains(message), "{error}");
        }

        let error = parse_data("bad.csv", "", b"0,1e308,1e308\n").unwrap_err();
        assert!(
            error.contains("row 1, column 3 produces non-finite"),
            "{error}"
        );
    }

    #[test]
    fn requires_two_or_three_consistent_columns_and_complete_headers() {
        for csv in [b"1\n".as_slice(), b"1,2,3,4\n"] {
            let error = parse_data("bad.csv", "", csv).unwrap_err();
            assert!(error.contains("expected 2 or 3"), "{error}");
        }
        for csv in [b"x,y,u_y\n0,1\n".as_slice(), b"0,1,0.1\n2,3\n"] {
            let error = parse_data("bad.csv", "", csv).unwrap_err();
            assert!(
                error.contains("row 2 contains 2 columns; expected exactly 3"),
                "{error}"
            );
        }
        let error = parse_data("bad.csv", "", b"time,ratio,0.1\n").unwrap_err();
        assert!(error.contains("row 1, column 1 is not a number"), "{error}");

        let error = parse_data("bad.csv", "", b"x,y,u_y\n").unwrap_err();
        assert!(error.contains("no data rows after its header"), "{error}");
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
            bytes: Some(Arc::from(&b"x,y,u_y\n0,1,0.1\n"[..])),
            ..Default::default()
        };

        let imported = import_dropped_file(&file).unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].points, vec![[0.0, 1.0]]);
        assert_eq!(imported[0].y_uncertainties, Some(vec![0.1]));
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
        std::fs::write(&path, b"x,y,u_y\n0,1,0.1\n").unwrap();
        let file = egui::DroppedFile {
            path: Some(path.clone()),
            ..Default::default()
        };

        let result = import_dropped_file(&file);
        std::fs::remove_file(path).unwrap();

        let imported = result.unwrap();
        assert_eq!(imported[0].points, vec![[0.0, 1.0]]);
        assert_eq!(imported[0].y_uncertainties, Some(vec![0.1]));
    }
}
