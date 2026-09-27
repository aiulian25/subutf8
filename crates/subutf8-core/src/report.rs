use std::io;
use std::path::PathBuf;

use crate::decoding::{ConversionFailure, ConversionWarning};
use crate::srt_structure::StructureWarning;

/// What happened to one file of a batch (UI-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Converted {
        output: PathBuf,
        warnings: Vec<ConversionWarning>,
        structure_warnings: Vec<StructureWarning>,
    },
    Skipped(SkipCause),
    Failed(FailureCause),
    /// Cancel stopped the batch before this file, which stays Ready (UI-06).
    NotReached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipCause {
    /// SAFE-04, skip setting.
    AlreadyExists,
    /// SAFE-05, overwrite setting: an earlier file in the list takes this name.
    NameTakenInList,
    /// SAFE-04: overwrite never replaces a file in the file list.
    ListedFile,
    /// SAFE-04 and SAFE-12.
    SymbolicLink,
    /// SAFE-04.
    ReadOnlyDestination,
    /// SAFE-03.
    FolderNotWritable,
    /// SAFE-11.
    NotRegularFile,
    /// NAME-06: every numbered name up to LIMIT-03 exists.
    NoFreeName,
    /// NAME-11: the name is not UTF-8 and does not decode in the file's encoding.
    NameNotDecodable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCause {
    /// ENC-01.
    Unreadable(io::ErrorKind),
    /// ENC-02.
    TooLarge,
    Conversion(ConversionFailure),
    /// NAME-09.
    NameTooLong,
    /// ENC-16.
    VerificationFailed,
    WriteFailed(io::ErrorKind),
}

/// UI-07: counts for the summary line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub converted: usize,
    pub skipped: usize,
    pub failed: usize,
    pub not_reached: usize,
}

impl Summary {
    pub fn of(outcomes: &[Outcome]) -> Self {
        let mut summary = Self::default();
        for outcome in outcomes {
            match outcome {
                Outcome::Converted { .. } => summary.converted += 1,
                Outcome::Skipped(_) => summary.skipped += 1,
                Outcome::Failed(_) => summary.failed += 1,
                Outcome::NotReached => summary.not_reached += 1,
            }
        }
        summary
    }
}
