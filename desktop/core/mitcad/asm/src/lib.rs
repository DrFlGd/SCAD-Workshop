// SPDX-License-Identifier: MIT
//! The ASM part of the `.f3d` reader: the ASM binary (and text) format of
//! the body blobs and its history, a neutral B-rep model in millimetres
//! with checks, and the conversion between them. The `.ipt` and `.dwg`
//! readers convert their B-rep records with it too.
//!
//! - [`asm`]: the ASM binary format of the body blobs (and the text form
//!   drawings keep), its history and a writer for tests.
//! - [`brep`]: a neutral B-rep model in millimetres with checks.
//! - [`convert`]: ASM bodies to the neutral model.
//! - [`flat`]: a neutral body as plain arrays for the C++ builder.
//! - [`ids`]: maps and sets keyed by record and entity indices.
//! - [`history_states`]: which states of a blob's history hold the same
//!   bodies (mitcad#675).
//!
//! The format knowledge comes from inspecting files only (black-box reverse
//! engineering); see `core/f3d/ASM_FORMAT.md`. Until mitcad#776 this was
//! part of `mitcad-f3d`, which re-exports it under the same paths.

pub mod asm;
pub mod brep;
pub mod convert;
pub mod flat;
pub mod history_states;
pub mod ids;
#[doc(hidden)]
pub mod testdata;

/// All bodies of one body blob.
pub struct BlobBodies {
    pub entry: String,
    pub header: asm::Header,
    pub coverage: asm::Coverage,
    /// Tokenising stopped early (unknown tag); the message.
    pub truncated: Option<String>,
    pub bodies: Vec<convert::ConvertedBody>,
}

/// Parses one body blob and converts its top-level bodies.
pub fn read_blob(
    entry: &str,
    data: &[u8],
    options: &convert::Options,
) -> Result<BlobBodies, asm::AsmError> {
    let file = asm::AsmFile::parse(data)?;
    let coverage = asm::Coverage::of(&file);
    let bodies = convert::convert_file(&file, options);
    Ok(BlobBodies {
        entry: entry.to_string(),
        header: file.header.clone(),
        coverage,
        truncated: file.truncated.as_ref().map(|e| e.to_string()),
        bodies,
    })
}
