extern crate clap;
extern crate csv;
extern crate handlebars;
extern crate itertools;
extern crate serde;
extern crate serde_json;

mod problems;

use std::env;
use std::fmt::Debug;
use std::fs::File;
use std::io::{self, Error, Write};
use std::path::Path;
use std::process::exit;
use std::time::SystemTime;

use clap::{arg_enum, value_t, App, Arg};
use handlebars::{Handlebars, RenderError, TemplateError};
use itertools::{EitherOrBoth, Itertools};

use problems::Problems;


arg_enum! {
    #[derive(PartialEq, Debug)]
    enum Delimiter {
        Comma,
        Pipe,
        Tab
    }
}

const DEFAULT_MAX_PROBLEMS: usize = 50;
const REPORT_TEMPLATE: &str = include_str!("../resources/report.html");

#[derive(Debug)]
enum ReportError {
    IO(io::Error),
    Render(RenderError),
    Template(TemplateError),
}

impl From<io::Error> for ReportError {
    fn from(error: Error) -> Self {
        ReportError::IO(error)
    }
}

impl From<RenderError> for ReportError {
    fn from(error: RenderError) -> Self {
        ReportError::Render(error)
    }
}

impl From<TemplateError> for ReportError {
    fn from(error: TemplateError) -> Self {
        ReportError::Template(error)
    }
}

fn generate_report<P: AsRef<Path>>(
    problems: &Problems,
    actual_filepath: &str,
    report_filepath: P,
) -> Result<(), ReportError> {
    let mut registry = Handlebars::new();
    registry.register_template_string("report", REPORT_TEMPLATE)?;
    let report_contents = registry.render("report", &problems.display_data(actual_filepath))?;
    let mut report_file = File::create(report_filepath)?;
    report_file.write_all(report_contents.as_bytes())?;
    Ok(())
}

#[derive(Debug)]
struct Summary {
    problems: Problems,
    errors: Vec<csv::Error>,
    max_problems: usize,
}

impl Summary {
    fn new(max_problems: Option<usize>) -> Summary {
        Summary {
            problems: Problems::new(max_problems.unwrap_or(DEFAULT_MAX_PROBLEMS)),
            errors: vec![],
            max_problems: max_problems.unwrap_or(DEFAULT_MAX_PROBLEMS),
        }
    }

    fn compare_line(
        &mut self,
        line_number: usize,
        expected_line: &csv::StringRecord,
        actual_line: &csv::StringRecord,
    ) {
        let mut column_number = 1;

        for cells in expected_line.iter().zip_longest(actual_line.iter()) {
            match cells {
                EitherOrBoth::Both(expected, actual) => {
                    if expected != actual {
                        self.problems
                            .insert_line_problem(problems::LineProblem::MismatchedCell {
                                line: line_number,
                                column: column_number,
                                expected: expected.to_string(),
                                actual: actual.to_string(),
                            });
                    }
                }
                EitherOrBoth::Left(_) => {
                    self.problems
                        .insert_line_problem(problems::LineProblem::MissingCell {
                            line: line_number,
                            column: column_number,
                        });
                }
                EitherOrBoth::Right(_) => {
                    self.problems
                        .insert_line_problem(problems::LineProblem::ExtraCell {
                            line: line_number,
                            column: column_number,
                        });
                }
            }

            column_number += 1;
        }
    }

    fn compare_lines(&mut self, rdr0: &mut csv::Reader<File>, rdr1: &mut csv::Reader<File>) {
        let mut line_number = 1;

        for lines in rdr0.records().zip_longest(rdr1.records()) {
            match lines {
                EitherOrBoth::Both(maybe_expected, maybe_actual) => {
                    match (maybe_expected, maybe_actual) {
                        (Ok(expected_line), Ok(actual_line)) => {
                            self.compare_line(line_number, &expected_line, &actual_line)
                        }
                        (Err(expected_error), Err(actual_error)) => {
                            self.errors.push(expected_error);
                            self.errors.push(actual_error);
                        }
                        (Err(error), _) => self.errors.push(error),
                        (_, Err(error)) => self.errors.push(error),
                    }
                }
                EitherOrBoth::Left(maybe_expected) => match maybe_expected {
                    Ok(_) => self.problems.insert_missing_lines_problem(line_number),
                    Err(error) => self.errors.push(error),
                },
                EitherOrBoth::Right(maybe_actual) => match maybe_actual {
                    Ok(_) => self.problems.insert_extra_lines_problem(line_number),
                    Err(error) => self.errors.push(error),
                },
            }

            if !self.errors.is_empty() {
                break;
            }

            line_number += 1;
        }
    }
}

fn handle_crash<T: Debug>(errors: &Vec<T>) {
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
    );

    exit(1);
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
            Arg::with_name("expected-delimiter")
                .short("e")
                .long("expected-delimiter")
                .value_name("DELIMITER")
                .help("Indicates the delimiter of the expected file.")
                .takes_value(true)
                .possible_values(&Delimiter::variants())
                .case_insensitive(true),
        )
        .arg(
            Arg::with_name("actual-delimiter")
                .short("a")
                .long("actual-delimiter")
                .value_name("DELIMITER")
                .help("Indicates the delimiter of the actual file.")
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
    let expected_delimiter =
        value_t!(matches, "expected-delimiter", Delimiter).unwrap_or(Delimiter::Comma);
    let actual_delimiter =
        value_t!(matches, "actual-delimiter", Delimiter).unwrap_or(Delimiter::Comma);

    match (
        get_reader(expected_filepath, expected_delimiter),
        get_reader(actual_filepath, actual_delimiter),
    ) {
        (Ok(ref mut rdr0), Ok(ref mut rdr1)) => {
            let mut summary = Summary::new(None);
            summary.compare_lines(rdr0, rdr1);
            if !summary.errors.is_empty() {
                handle_crash(&summary.errors);
            }

            if let Err(report_error) =
                generate_report(&summary.problems, actual_filepath, "out.html")
            {
                handle_crash(&vec![report_error]);
            }
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
                handle_crash(&errors);
            }
        }
        (Err(e), _) => {
            let mut errors = vec![];
            if let Err(error) = handle_failed_reader(e, expected_filepath) {
                errors.push(error);
            }
            if !errors.is_empty() {
                handle_crash(&errors);
            }
        }
        (_, Err(e)) => {
            let mut errors = vec![];
            if let Err(error) = handle_failed_reader(e, actual_filepath) {
                errors.push(error);
            }
            if !errors.is_empty() {
                handle_crash(&errors);
            }
        }
    }
}
