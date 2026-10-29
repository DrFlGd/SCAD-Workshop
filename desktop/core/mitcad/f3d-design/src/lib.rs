// SPDX-License-Identifier: MIT
//! The design segment: the object database with the timeline,
//! parameters, sketches, features, components and occurrences.
//!
//! Layers:
//! - [`stream`]: `MetaStream.dat` and the object framing of
//!   `BulkStream.dat` (headers, root parts, references, strings).
//! - [`decode`] and [`sketch`]: decoders for the parts that the timeline
//!   format study marks certain or probable.
//! - [`build`]: the decoded data shaped as the dump IR ([`ir`]), the JSON
//!   of `core/import/SCHEMA.md`.
//! - [`recipe`] and [`inputs`]: feature inputs named as the bodies name
//!   their faces and edges, found in the ASM history of the body blobs
//!   ([`decode_document`] does it).
//!
//! The decoder reproduces the reference decoder (a separate Python
//! implementation of the format study, not part of this repository)
//! exactly, so that the two can be compared on the corpus
//! (`core/f3d/tests/design_corpus.rs`). Fields that cannot be decoded are
//! absent, never guessed.
//!
//! Until mitcad#776 this was the module `design` of `mitcad-f3d`, which
//! re-exports it as `mitcad_f3d::design`. Since mitcad#779 the decoding
//! layers ([`stream`], [`decode`], [`sketch`], [`classes`], [`recipe`],
//! [`curve`], [`learn`], [`writer`]) are the crate `mitcad-f3d-decode`,
//! re-exported here under their old paths.
//!
//! ```no_run
//! use mitcad_f3d_design as design;
//! for d in design::decode_path(std::path::Path::new("part.f3d")).unwrap() {
//!     let dump = d.dump.expect("design segment");
//!     for item in dump.timeline_items() {
//!         println!("{:?} {:?}", item.name(), item.object_type());
//!     }
//! }
//! ```

// Appearances and materials of bodies and faces (mitcad#228).
pub mod appearance;
pub mod build;
pub mod inputs;
// The dump IR is the `mitcad-dump` crate (mitcad#776).
pub use mitcad_dump as ir;
mod nonfinite;
// What an `.f3z` package says about its designs (mitcad#229).
pub mod package;
// Where derives' placements without a matrix are (mitcad#324).
pub mod derive_sources;
// The streams, records and sketches are decoded by `mitcad-f3d-decode`
// (mitcad#779), re-exported here under their old paths: the learning
// dump's raw records (mitcad#96), the synthetic design streams for tests
// (mitcad#196) and the curves edge inputs store beside their recipes
// (mitcad#306) among them.
pub use mitcad_f3d_decode::{classes, curve, decode, learn, recipe, sketch, stream, writer};

// Python's character tables, for the names `build` keeps.
use mitcad_f3d_decode::unicode;

use std::path::Path;

pub use ir::Dump;
pub use stream::{MetaError, MetaStream, Segment};

use mitcad_f3d_file::container::F3dFile;
use mitcad_zip::ZipError;

/// Folder names of the design segment.
pub const DESIGN_SEGMENTS: [&str; 2] = ["Design1", "FusionDesignSegmentType1"];

/// The design segment's streams.
#[derive(Clone, Debug)]
pub struct DesignStreams {
    /// `Design1` or `FusionDesignSegmentType1`.
    pub segment_dir: String,
    pub meta: Vec<u8>,
    pub bulk: Vec<u8>,
}

/// The design segment of a document (`FusionAssetName.../Design1/` or
/// `.../FusionDesignSegmentType1/`), the first in archive order.
pub fn find_design_streams(doc: &F3dFile) -> Result<Option<DesignStreams>, ZipError> {
    for e in doc.archive().entries() {
        let parts: Vec<&str> = e.name.split('/').collect();
        if parts.len() == 3
            && parts[0].starts_with("FusionAssetName")
            && DESIGN_SEGMENTS.contains(&parts[1])
            && parts[2] == "MetaStream.dat"
        {
            let meta = doc.archive().read(e)?;
            let bulk = doc.read(&format!("{}/{}/BulkStream.dat", parts[0], parts[1]))?;
            return Ok(Some(DesignStreams {
                segment_dir: parts[1].to_string(),
                meta,
                bulk,
            }));
        }
    }
    Ok(None)
}

/// A decoded design segment.
pub struct Design {
    pub segment: Segment,
    pub decoded: decode::Decoded,
}

impl Design {
    /// Parses the streams and decodes them.
    pub fn parse(meta: &[u8], bulk: Vec<u8>) -> Result<Design, MetaError> {
        let segment = Segment::parse(meta, bulk)?;
        let decoded = decode::decode(&segment);
        Ok(Design { segment, decoded })
    }

    /// The dump IR. `file` and `segment_dir` are written to `source`.
    pub fn dump(&self, file: &str, segment_dir: &str) -> Dump {
        build::build(&self.segment, &self.decoded, file, segment_dir)
    }
}

/// One design of a file (an `.f3z` package holds several).
pub struct FileDesign {
    /// `name.f3d`, or `package.f3z!<first 8 characters of the entry>`.
    pub label: String,
    /// The design segment folder, if the document has one.
    pub segment_dir: Option<String>,
    /// The dump; `None` if there is no design segment.
    pub dump: Option<Dump>,
}

/// The file name without a cache GUID
/// (`name.<36 characters>.f3d` -> `name.f3d`).
pub fn short_name(path: &Path) -> String {
    let b = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parts: Vec<&str> = b.split('.').collect();
    if parts.len() >= 3 && parts[parts.len() - 2].chars().count() == 36 {
        format!(
            "{}.{}",
            parts[..parts.len() - 2].join("."),
            parts[parts.len() - 1]
        )
    } else {
        b
    }
}

/// The documents of a file with their labels: the file itself, or the
/// `.f3d` entries of an `.f3z` package.
pub fn documents(path: &Path) -> Result<Vec<(String, F3dFile)>, String> {
    package(path).map(|p| p.documents)
}

/// The documents of a file ([`documents`]) and what an `.f3z` package says
/// about them (mitcad#229, [`package::PackageDescription`]).
pub struct Package {
    /// The documents with their labels.
    pub documents: Vec<(String, F3dFile)>,
    /// The root design's label, where `Manifest.json` names it.
    pub root: Option<String>,
    /// The designs `DesignDescription.json` lists that the package holds,
    /// with their labels.
    pub described: Vec<(String, package::DescribedDesign)>,
}

/// [`documents`] with what an `.f3z` package says about them.
pub fn package(path: &Path) -> Result<Package, String> {
    let f = F3dFile::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = short_name(path);
    let is_package = path.to_string_lossy().to_lowercase().ends_with(".f3z");
    if !is_package {
        return Ok(Package {
            documents: vec![(name, f)],
            root: None,
            described: Vec::new(),
        });
    }
    let description = package::PackageDescription::read(&f);
    let mut out = Package {
        documents: Vec::new(),
        root: None,
        described: Vec::new(),
    };
    for e in f.archive().entries() {
        if !e.name.to_lowercase().ends_with(".f3d") {
            continue;
        }
        let data = f
            .archive()
            .read(e)
            .map_err(|err| format!("{}!{}: {err}", path.display(), e.name))?;
        let doc = F3dFile::from_bytes(data)
            .map_err(|err| format!("{}!{}: {err}", path.display(), e.name))?;
        let prefix: String = e.name.chars().take(8).collect();
        let label = format!("{name}!{prefix}");
        if description
            .root
            .as_deref()
            .is_some_and(|r| package::is_entry(&e.name, r))
        {
            out.root = Some(label.clone());
        }
        for d in &description.designs {
            if package::is_entry(&e.name, &d.path) {
                out.described.push((label.clone(), d.clone()));
            }
        }
        out.documents.push((label, doc));
    }
    Ok(out)
}

/// Decodes the design of a document.
pub fn decode_document(doc: &F3dFile, label: &str) -> Result<FileDesign, String> {
    let mut design = decode_streams(doc, label)?;
    resolve_inputs(&mut design, doc, 1);
    Ok(design)
}

// Decoding in two parts (mitcad#103): the import decodes every design's
// streams to choose one, and finds the chosen one's inputs in the bodies'
// history while it reads the bodies themselves.

/// [`decode_document`] without [`resolve_inputs`]: the design streams
/// alone (the timeline, components, features and the blobs' links); quick
/// next to finding the inputs in the bodies' history.
pub fn decode_streams(doc: &F3dFile, label: &str) -> Result<FileDesign, String> {
    let Some(s) = find_design_streams(doc).map_err(|e| format!("{label}: {e}"))? else {
        return Ok(FileDesign {
            label: label.to_string(),
            segment_dir: None,
            dump: None,
        });
    };
    let design = Design::parse(&s.meta, s.bulk).map_err(|e| format!("{label}: {e}"))?;
    let mut dump = design.dump(label, &s.segment_dir);
    // The appearances and materials (mitcad#228); their faces and the
    // bodies' records in the blobs come with the inputs.
    dump.f3d.get_or_insert_with(Default::default).appearance =
        Some(appearance::decode(&design.segment, doc));
    Ok(FileDesign {
        label: label.to_string(),
        dump: Some(dump),
        segment_dir: Some(s.segment_dir),
    })
}

/// The rest of [`decode_document`] after [`decode_streams`]: the inputs
/// named in the streams, found in the bodies' history (on large designs
/// most of the decoding's time). Changes the items' inputs only, not the
/// timeline's order, the items' states and components, the components or
/// the blobs' links. `threads`: how many threads find them (the items one
/// at a time each; the same dump with any number).
pub fn resolve_inputs(design: &mut FileDesign, doc: &F3dFile, threads: usize) {
    if let Some(dump) = &mut design.dump {
        inputs::resolve_on(dump, doc, threads);
        if let Some(a) = dump.f3d.as_mut().and_then(|f| f.appearance.as_mut()) {
            appearance::resolve(a, doc, threads);
        }
    }
}

/// [`resolve_inputs`] for the entities of other documents the design
/// names (mitcad#196): `documents` are those documents' files with the GUIDs
/// the design's inputs name them with (`_f3d.document`) and the design's keys
/// for them; their entities are found in the last state of those files'
/// histories.
pub fn resolve_external_inputs(design: &mut FileDesign, documents: &[inputs::ExternalFile<'_>]) {
    if let Some(dump) = &mut design.dump {
        inputs::resolve_external(dump, documents);
    }
}

/// The placements of the design's derives that keep no matrix, from their
/// source designs' occurrence trees (mitcad#324): `documents` as for
/// [`resolve_external_inputs`], the source designs' files among them.
pub fn resolve_derive_placements(
    design: &mut FileDesign,
    documents: &[inputs::ExternalFile<'_>],
) -> usize {
    design
        .dump
        .as_mut()
        .map_or(0, |dump| derive_sources::resolve(dump, documents))
}

/// Decodes every design of an `.f3d` or `.f3z` file.
pub fn decode_path(path: &Path) -> Result<Vec<FileDesign>, String> {
    documents(path)?
        .iter()
        .map(|(label, doc)| decode_document(doc, label))
        .collect()
}

#[cfg(test)]
mod tests;

/// The GUIDs (lower case) the `Manifest.dat` files of a document list: an
/// external document's `document_guid` (`ir::ExternalDocument`) is one of
/// those of the file it refers to (mitcad#196).
pub fn manifest_guids(doc: &F3dFile) -> Vec<String> {
    let mut out = Vec::new();
    for e in doc.archive().entries() {
        if e.name.rsplit('/').next() == Some("Manifest.dat")
            && let Ok(data) = doc.archive().read(e)
        {
            out.extend(build::guid_strings(&data));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// A document a design refers to (an external document of its design
/// segment, mitcad#196), as [`referenced_documents`] lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferencedDocument {
    /// The key references to it carry (16 hex digits).
    pub key: String,
    pub urn: String,
    /// The version the URN names.
    pub version: Option<u32>,
    /// One of the GUIDs of the referenced file's `Manifest.dat`.
    pub document_guid: String,
    /// The design's occurrences of its components.
    pub occurrences: usize,
}

/// The documents a design refers to, with how many occurrences of their
/// components it has, read from the design streams without decoding the
/// design (quick: for asking where they are before an import).
pub fn referenced_documents(doc: &F3dFile) -> Result<Vec<ReferencedDocument>, String> {
    let Some(s) = find_design_streams(doc).map_err(|e| e.to_string())? else {
        return Ok(Vec::new());
    };
    let segment = Segment::parse(&s.meta, s.bulk).map_err(|e| e.to_string())?;
    let mut counts: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for o in segment.objects_of(classes::OCCURRENCE) {
        if let Some(key) = build::occurrence_info(&segment, o.id).and_then(|i| i.context) {
            *counts.entry(key).or_default() += 1;
        }
    }
    Ok(segment
        .meta
        .external
        .iter()
        .map(|x| ReferencedDocument {
            key: format!("{:016x}", x.key),
            urn: x.urn.clone(),
            version: build::urn_version(&x.urn),
            document_guid: x.guid_c.to_lowercase(),
            occurrences: counts.get(&x.key).copied().unwrap_or(0),
        })
        .collect())
}
