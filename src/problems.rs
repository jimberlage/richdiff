use std::cmp::min;
use std::collections::HashSet;

use itertools::Itertools;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum ProblemCategory {
    MismatchedCells,
    ExtraCells,
    MissingCells,
    ExtraLines,
    MissingLines,
}

impl Serialize for ProblemCategory {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut category = serializer.serialize_map(Some(3))?;
        match self {
            Self::MismatchedCells => {
                category.serialize_entry("type", "Mismatched cells")?;
                category.serialize_entry("color", "red")?;
                category.serialize_entry(
                    "description",
                    "The contents of one or more cells in the actual file did not match up.",
                )?;
            }
            Self::ExtraCells => {
                category.serialize_entry("type", "Extra cells")?;
                category.serialize_entry("color", "orange")?;
                category.serialize_entry(
                    "description",
                    "A line (or lines) in the actual file had more cells than expected.",
                )?;
            }
            Self::MissingCells => {
                category.serialize_entry("type", "Missing cells")?;
                category.serialize_entry("color", "yellow")?;
                category.serialize_entry(
                    "description",
                    "A line (or lines) in the actual file is missing cells.",
                )?;
            }
            Self::ExtraLines => {
                category.serialize_entry("type", "Extra lines")?;
                category.serialize_entry("color", "green")?;
                category.serialize_entry(
                    "description",
                    "The actual file had more lines in it than expected.",
                )?;
            }
            Self::MissingLines => {
                category.serialize_entry("type", "Missing lines")?;
                category.serialize_entry("color", "blue")?;
                category.serialize_entry(
                    "description",
                    "The actual file had fewer lines in it than expected.",
                )?;
            }
        };
        category.end()
    }
}

#[derive(Debug, Clone)]
pub enum LineProblem {
    MismatchedCell {
        line: usize,
        column: usize,
        expected: String,
        actual: String,
    },
    ExtraCell {
        line: usize,
        column: usize,
    },
    MissingCell {
        line: usize,
        column: usize,
    },
}

#[derive(Debug, Clone)]
pub struct ExtraLinesProblem {
    line: usize,
    num_extra: usize,
}

#[derive(Debug, Clone)]
pub struct MissingLinesProblem {
    line: usize,
    num_missing: usize,
}

#[derive(Debug)]
pub enum FileProblem {
    ExtraLines(ExtraLinesProblem),
    MissingLines(MissingLinesProblem),
}

#[derive(Debug)]
pub enum Problem {
    Line(LineProblem),
    File(FileProblem),
}

impl Problem {
    pub fn category(&self) -> ProblemCategory {
        match self {
            Self::Line(LineProblem::MismatchedCell {
                line: _,
                column: _,
                expected: _,
                actual: _,
            }) => ProblemCategory::MismatchedCells,
            Self::Line(LineProblem::ExtraCell { line: _, column: _ }) => {
                ProblemCategory::ExtraCells
            }
            Self::Line(LineProblem::MissingCell { line: _, column: _ }) => {
                ProblemCategory::MissingCells
            }
            Self::File(FileProblem::ExtraLines(_)) => ProblemCategory::ExtraLines,
            Self::File(FileProblem::MissingLines(_)) => ProblemCategory::MissingLines,
        }
    }
}

impl Serialize for Problem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut problem = serializer.serialize_map(Some(3))?;
        match self {
            Self::Line(LineProblem::MismatchedCell {
                line,
                column,
                expected,
                actual,
            }) => {
                problem.serialize_entry("type", "Mismatched cell")?;
                problem.serialize_entry("color", "red")?;
                problem.serialize_entry(
                    "description",
                    &format!(
                        "The cell at line {}, column {} was {}, but the expected value was {}.",
                        line, column, actual, expected
                    ),
                )?;
            }
            Self::Line(LineProblem::ExtraCell { line, column }) => {
                problem.serialize_entry("type", "Extra cell")?;
                problem.serialize_entry("color", "orange")?;
                problem.serialize_entry(
                    "description",
                    &format!(
                        "The cell at line {}, column {} is not present in the expected file.",
                        line, column
                    ),
                )?;
            }
            Self::Line(LineProblem::MissingCell { line, column }) => {
                problem.serialize_entry("type", "Missing cell")?;
                problem.serialize_entry("color", "yellow")?;
                problem.serialize_entry(
                    "description",
                    &format!("A cell is missing at line {}, column {}.", line, column),
                )?;
            }
            Self::File(FileProblem::ExtraLines(ExtraLinesProblem { line, num_extra })) => {
                problem.serialize_entry("type", "Extra line")?;
                problem.serialize_entry("color", "green")?;
                problem.serialize_entry(
                    "description",
                    &format!(
                        "There were {} extra lines, starting with line {}.",
                        num_extra, line
                    ),
                )?;
            }
            Self::File(FileProblem::MissingLines(MissingLinesProblem { line, num_missing })) => {
                problem.serialize_entry("type", "Missing line")?;
                problem.serialize_entry("color", "blue")?;
                problem.serialize_entry(
                    "description",
                    &format!(
                        "There were {} lines missing, ending at line {}.",
                        num_missing, line
                    ),
                )?;
            }
        };
        problem.end()
    }
}

#[derive(Debug, Serialize)]
pub struct DisplayProblems {
    actual_filename: String,
    num_problems: usize,
    found_max_problems: bool,
    problem_categories: Vec<ProblemCategory>,
    problems: Vec<Problem>,
}

#[derive(Debug)]
pub struct Problems {
    max_problems_to_display: usize,
    extra_lines_problem: Option<ExtraLinesProblem>,
    missing_lines_problem: Option<MissingLinesProblem>,
    line_problems: Vec<LineProblem>,
}

pub struct DisplayableProblems<I> {
    line_problems_to_display: usize,
    extra_lines_problem: Option<ExtraLinesProblem>,
    missing_lines_problem: Option<MissingLinesProblem>,
    iter: I,
}

impl<'a, I> Iterator for DisplayableProblems<I>
where
    I: Iterator<Item = &'a LineProblem>,
{
    type Item = Problem;

    fn next(&mut self) -> Option<Self::Item> {
        if self.line_problems_to_display > 0 {
            self.line_problems_to_display -= 1;
            self.iter
                .next()
                .map(|line_problem| Problem::Line(line_problem.clone()))
        } else if let Some(extra_lines_problem) = &self.extra_lines_problem {
            Some(Problem::File(FileProblem::ExtraLines(
                extra_lines_problem.clone(),
            )))
        } else if let Some(missing_lines_problem) = &self.missing_lines_problem {
            Some(Problem::File(FileProblem::MissingLines(
                missing_lines_problem.clone(),
            )))
        } else {
            None
        }
    }
}

impl Problems {
    pub fn new(max_problems_to_display: usize) -> Self {
        Problems {
            max_problems_to_display,
            extra_lines_problem: None,
            missing_lines_problem: None,
            line_problems: vec![],
        }
    }

    pub fn len(&self) -> usize {
        self.line_problems.len()
            + self.extra_lines_problem.as_ref().map(|_| 1).unwrap_or(0)
            + self.missing_lines_problem.as_ref().map(|_| 1).unwrap_or(0)
    }

    pub fn insert_extra_lines_problem(&mut self, line: usize) {
        match &mut self.extra_lines_problem {
            None => {
                self.extra_lines_problem = Some(ExtraLinesProblem { line, num_extra: 1 });
            }
            Some(ref mut extra_lines_problem) => {
                extra_lines_problem.num_extra += 1;
            }
        }
    }

    pub fn insert_missing_lines_problem(&mut self, line: usize) {
        match &mut self.missing_lines_problem {
            None => {
                self.missing_lines_problem = Some(MissingLinesProblem {
                    line,
                    num_missing: 1,
                });
            }
            Some(ref mut missing_lines_problem) => {
                missing_lines_problem.num_missing += 1;
            }
        }
    }

    pub fn insert_line_problem(&mut self, problem: LineProblem) {
        self.line_problems.push(problem);
    }

    pub fn displayable_problems(&self) -> DisplayableProblems<std::slice::Iter<LineProblem>> {
        let line_problems_to_display = min(
            self.line_problems.len() as usize,
            self.max_problems_to_display - (self.len() - self.line_problems.len()),
        );
        DisplayableProblems {
            line_problems_to_display,
            extra_lines_problem: self.extra_lines_problem.clone(),
            missing_lines_problem: self.missing_lines_problem.clone(),
            iter: self.line_problems.iter(),
        }
    }

    pub fn display_data(&self, actual_filename: &str) -> DisplayProblems {
        let mut categories = HashSet::new();
        let mut problems = vec![];

        for problem in self.displayable_problems() {
            categories.insert(problem.category());
            problems.push(problem);
        }

        DisplayProblems {
            actual_filename: actual_filename.to_string(),
            num_problems: self.len(),
            found_max_problems: self.len() >= self.max_problems_to_display,
            problem_categories: categories.iter().sorted().cloned().collect(),
            problems,
        }
    }
}
