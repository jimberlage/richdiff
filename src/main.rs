extern crate clap;
extern crate csv;
extern crate itertools;

use std::env;
use std::fmt::Debug;
use std::fs::File;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use clap::{arg_enum, value_t, App, Arg};
use itertools::{EitherOrBoth, Itertools};
use std::io::Write;

arg_enum! {
    #[derive(PartialEq, Debug)]
    enum Delimiter {
        Comma,
        Pipe,
        Tab
    }
}

const DEFAULT_MAX_CHANGES: u64 = 50;

enum FileType {
    Expected,
    Actual,
}

enum Change {
    MismatchedCell {
        line: u64,
        column: u64,
        expected: String,
        actual: String,
    },
    ExtraCell {
        line: u64,
        column: u64,
    },
    MissingCell {
        line: u64,
        column: u64,
    },
    ExtraLine {
        line: u64,
    },
    MissingLine {
        line: u64,
    },
}

struct Metadata {
    changes: Vec<Change>,
    errors: Vec<csv::Error>,
    line_lengths: (u64, u64),
    max_changes: u64,
}

impl Metadata {
    fn new(max_changes: Option<u64>) -> Metadata {
        Metadata {
            changes: vec![],
            errors: vec![],
            line_lengths: (0u64, 0u64),
            max_changes: max_changes.unwrap_or(DEFAULT_MAX_CHANGES),
        }
    }

    fn compare_line(
        &mut self,
        line_number: u64,
        expected_line: csv::StringRecord,
        actual_line: csv::StringRecord,
    ) {
        let mut column_number = 1;

        for cells in expected_line.iter().zip_longest(actual_line.iter()) {
            match cells {
                EitherOrBoth::Both(expected, actual) => {
                    if expected != actual {
                        self.changes.push(Change::MismatchedCell {
                            line: line_number,
                            column: column_number,
                            expected: expected.to_string(),
                            actual: actual.to_string(),
                        });
                    }
                }
                EitherOrBoth::Left(expected) => {
                    self.changes.push(Change::MissingCell {
                        line: line_number,
                        column: column_number,
                    });
                }
                EitherOrBoth::Right(actual) => {
                    self.changes.push(Change::ExtraCell {
                        line: line_number,
                        column: column_number,
                    });
                }
            }

            column_number += 1;

            if self.changes.len() >= self.max_changes as usize {
                break;
            }
        }
    }

    fn compare_lines(&mut self, rdr0: &mut csv::Reader<File>, rdr1: &mut csv::Reader<File>) {
        let mut line_number = 1;

        for lines in rdr0.records().zip_longest(rdr1.records()) {
            match lines {
                EitherOrBoth::Both(maybe_expected, maybe_actual) => {
                    self.line_lengths.0 += 1;
                    self.line_lengths.1 += 1;

                    match (maybe_expected, maybe_actual) {
                        (Ok(expected_line), Ok(actual_line)) => {
                            self.compare_line(line_number, expected_line, actual_line)
                        }
                        (Err(expected_error), Err(actual_error)) => {
                            self.errors.push(expected_error);
                            self.errors.push(actual_error);
                        }
                        (Err(error), _) => self.errors.push(error),
                        (_, Err(error)) => self.errors.push(error),
                    }
                }
                EitherOrBoth::Left(maybe_expected) => {
                    self.line_lengths.0 += 1;

                    match maybe_expected {
                        Ok(_) => (),
                        Err(error) => self.errors.push(error)
                    }
                }
                EitherOrBoth::Right(maybe_actual) => {
                    self.line_lengths.1 += 1;

                    match maybe_actual {
                        Ok(_) => (),
                        Err(error) => self.errors.push(error),
                    }
                }
            }

            if !self.errors.is_empty() || self.changes.len() >= self.max_changes as usize {
                break;
            }

            line_number += 1;
        }
    }
}

fn handle_crash<T: Debug>(errors: Vec<T>) {
    let mut log_filepath = env::temp_dir();
    log_filepath.push(format!("richdiff_crash_{:?}.log", SystemTime::now()));
    let mut log_file = File::create(log_filepath.clone()).unwrap();
    log_file
        .write_all(
            errors
                .iter()
                .map(|error| format!("{:?}", error))
                .join("\n")
                .as_ref(),
        )
        .unwrap();

    eprintln!(
        "An unexpected error occurred.  You can check the log at\n\n{:?}",
        log_filepath
    )
}

fn get_reader<P: AsRef<Path>>(filepath: P, delimiter: Delimiter) -> csv::Result<csv::Reader<File>> {
    let delimiter_byte = match delimiter {
        Delimiter::Comma => b',',
        Delimiter::Pipe => b'|',
        Delimiter::Tab => b'\t',
    };
    csv::ReaderBuilder::new()
        // With the expected file as the source of truth, we can't assume that it has a consistent number of rows.
        // The flexible option ensures that doesn't surface as an error.
        .flexible(true)
        .delimiter(delimiter_byte)
        .from_path(filepath)
}

fn handle_failed_reader(error: csv::Error, file: &str) -> Result<(), csv::Error> {
    match error.kind() {
        csv::ErrorKind::Io(io_error) => match io_error.kind() {
            io::ErrorKind::NotFound => Ok(eprintln!(
                "{} does not exist - did you mistype the file name?",
                file
            )),
            io::ErrorKind::PermissionDenied => {
                Ok(eprintln!("{} cannot be read due to its permissions.", file))
            }
            _ => Err(error),
        },
        _ => Err(error),
    }
}

fn main() {
    let matches = App::new("richdiff")
        .version("1.0")
        .author("Jim Berlage <james.berlage@gmail.com>")
        .about("Provides a rich diff of changes between two large CSVs.")
        .arg(
            Arg::with_name("delimiter0")
                .short("d")
                .long("delimiter0")
                .value_name("DELIMITER")
                .help("Indicates the delimiter of the first file.")
                .takes_value(true)
                .possible_values(&Delimiter::variants())
                .case_insensitive(true),
        )
        .arg(
            Arg::with_name("delimiter1")
                .short("e")
                .long("delimiter1")
                .value_name("DELIMITER")
                .help("Indicates the delimiter of the second file.")
                .takes_value(true)
                .possible_values(&Delimiter::variants())
                .case_insensitive(true),
        )
        .arg(
            Arg::with_name("EXPECTED")
                .help("The path to the file that is the source of truth.")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("ACTUAL")
                .help("The path to the file that needs to look like the source of truth.")
                .required(true)
                .index(2),
        )
        .get_matches();

    let expected_filepath = matches.value_of("EXPECTED").unwrap();
    let actual_filepath = matches.value_of("ACTUAL").unwrap();
    let delimiter0 = value_t!(matches, "delimiter0", Delimiter).unwrap_or(Delimiter::Comma);
    let delimiter1 = value_t!(matches, "delimiter1", Delimiter).unwrap_or(Delimiter::Comma);

    match (
        get_reader(expected_filepath, delimiter0),
        get_reader(actual_filepath, delimiter1),
    ) {
        (Ok(ref mut rdr0), Ok(ref mut rdr1)) => {
            let mut metadata = Metadata::new(None);
            metadata.compare_lines(rdr0, rdr1);
            ()
        }
        (Err(e0), Err(e1)) => {
            let mut errors = vec![];
            if let Err(error) = handle_failed_reader(e0, expected_filepath) {
                errors.push(error);
            }
            if let Err(error) = handle_failed_reader(e1, actual_filepath) {
                errors.push(error);
            }
            if !errors.is_empty() {
                handle_crash(errors);
            }
        }
        (Err(e), _) => {
            let mut errors = vec![];
            if let Err(error) = handle_failed_reader(e, expected_filepath) {
                errors.push(error);
            }
            if !errors.is_empty() {
                handle_crash(errors);
            }
        }
        (_, Err(e)) => {
            let mut errors = vec![];
            if let Err(error) = handle_failed_reader(e, actual_filepath) {
                errors.push(error);
            }
            if !errors.is_empty() {
                handle_crash(errors);
            }
        }
    }
}
