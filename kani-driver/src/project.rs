// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! This module defines the structure for a Kani project.

use crate::metadata::{from_json, mock_proof_harness};
use crate::session::KaniSession;
use crate::util::{crate_name, guess_rlib_name};
use anyhow::Result;
use kani_metadata::{ArtifactType, HarnessMetadata, KaniMetadata};
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use tracing::debug;

/// This structure represent the project information relevant for verification.
/// For dry-run, this structure is populated with mock data.
#[derive(Debug, Default)]
pub struct Project {
    /// The directory where all outputs should be directed to.
    pub outdir: PathBuf,
    /// The collection of artifacts kept as part of this project.
    pub artifacts: Vec<Artifact>,
    /// Each target crate metadata.
    pub metadata: Vec<KaniMetadata>,
}

impl Project {
    pub fn get_artifacts(&self, typ: ArtifactType) -> Vec<Artifact> {
        self.artifacts.iter().filter(|artifact| artifact.has_type(typ)).cloned().collect()
    }

    pub fn get_all_harnesses(&self) -> Vec<&HarnessMetadata> {
        self.metadata
            .iter()
            .flat_map(|crate_metadata| {
                crate_metadata.proof_harnesses.iter().chain(crate_metadata.test_harnesses.iter())
            })
            .collect()
    }

    // TODO: Should we create a HarnessId instead of using metadata everywhere?
    pub fn get_harness_artifact(
        &self,
        harness: &HarnessMetadata,
        typ: ArtifactType,
    ) -> Option<&Artifact> {
        self.artifacts.iter().find(|artifact| {
            artifact.has_type(typ)
                && artifact.harness_mangled.as_ref() == Some(&harness.mangled_name)
        })
    }

    /// Return the matching artifact for the given crate_name. If more than one artifact is found,
    /// this will return the first element.
    pub fn get_crate_artifact(&self, crate_name: &String, typ: ArtifactType) -> Option<&Artifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.has_type(typ) && artifact.crate_name == *crate_name)
    }

    pub fn get_crate_artifacts(&self, crate_name: &String, typ: ArtifactType) -> Vec<&Artifact> {
        self.artifacts
            .iter()
            .filter(|artifact| artifact.has_type(typ) && artifact.crate_name == *crate_name)
            .collect()
    }
}

// Information about a build artifact.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Artifact {
    /// The name of the harness that this artifact is relative to a harness, if that's the case.
    harness_mangled: Option<String>,
    /// The name of the crate that originated this artifact.
    crate_name: String,
    /// The path for this artifact.
    path: PathBuf,
    /// The type of artifact.
    typ: ArtifactType,
}

impl AsRef<Path> for Artifact {
    fn as_ref(&self) -> &Path {
        self.path.as_ref()
    }
}

impl Deref for Artifact {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl Artifact {
    fn has_type(&self, typ: ArtifactType) -> bool {
        self.typ == typ
    }
}

fn cargo_artifact(metadata: &Path, crate_name: &str, typ: ArtifactType) -> Option<Artifact> {
    let path = metadata.with_extension(&typ);
    if path.exists() {
        Some(Artifact { harness_mangled: None, crate_name: crate_name.to_string(), path, typ })
    } else {
        None
    }
}

pub fn cargo_project(session: &KaniSession) -> Result<Project> {
    let outputs = session.cargo_build()?;
    let mut metadata = vec![];
    let mut artifacts = vec![];
    if session.args.dry_run {
        // Just fake a project and a call to the linker.
        todo!()
    } else if session.args.legacy_linker {
        // For the legacy linker or `--function` support, we still use a glob to link everything.
        // Yes, this is broken, but it has been broken for quite some time.
        todo!()
    } else {
        // For the MIR Linker we know there is only one artifact per verification target. Use
        // that in our favor.
        for meta_file in outputs.metadata {
            // Link the artifact.
            let base_path = meta_file.parent().unwrap().join(meta_file.file_stem().unwrap());
            let symtab_out = base_path.with_extension(&ArtifactType::SymTabGoto);
            let goto = base_path.with_extension(&ArtifactType::Goto);
            session.link_goto_binary(&[symtab_out], &goto)?;

            // Store project information.
            let crate_metadata: KaniMetadata = from_json(&meta_file)?;
            let crate_name = &crate_metadata.crate_name;
            artifacts.extend(
                BUILD_ARTIFACTS
                    .iter()
                    .filter_map(|typ| cargo_artifact(&base_path, crate_name, *typ)),
            );
            debug!(?crate_name, ?crate_metadata, "cargo_project");
            metadata.push(crate_metadata);
        }
        Ok(Project { outdir: outputs.outdir, artifacts, metadata })
    }
}

pub struct StandaloneProjectBuilder<'a> {
    /// The directory where all outputs should be directed to.
    outdir: PathBuf,
    /// The collection of artifacts that may be generated.
    artifacts: HashMap<ArtifactType, Artifact>,
    /// The input file.
    input: PathBuf,
    /// The crate name.
    crate_name: String,
    /// The Kani session.
    session: &'a KaniSession,
}

/// All the type of artifacts that may be generated as part of the build.
const BUILD_ARTIFACTS: [ArtifactType; 6] = [
    ArtifactType::Metadata,
    ArtifactType::Goto,
    ArtifactType::SymTab,
    ArtifactType::SymTabGoto,
    ArtifactType::TypeMap,
    ArtifactType::VTableRestriction,
];

impl<'a> StandaloneProjectBuilder<'a> {
    pub fn try_new(input: &Path, session: &'a KaniSession) -> Result<Self> {
        let outdir = session
            .args
            .target_dir
            .clone()
            .unwrap_or_else(|| input.canonicalize().unwrap().parent().unwrap().to_owned());
        let crate_name = crate_name(&input);
        let artifacts =
            BUILD_ARTIFACTS.map(|typ| (typ, standalone_artifact(&outdir, &crate_name, typ)));
        Ok(StandaloneProjectBuilder {
            outdir,
            artifacts: HashMap::from(artifacts),
            input: input.to_path_buf(),
            crate_name,
            session,
        })
    }

    pub fn build(self) -> Result<Project> {
        // Register artifacts that may be generated by the compiler / linker for future deletion.
        let rlib_path = guess_rlib_name(&self.outdir.join(self.input.file_name().unwrap()));
        self.session.record_temporary_files(&[&rlib_path]);
        self.session.record_temporary_files(&self.artifacts.values().collect::<Vec<_>>());

        // Build and link the artifacts.
        debug!(krate=?self.crate_name, input=?self.input, ?rlib_path, "build compile");
        self.session.compile_single_rust_file(&self.input, &self.crate_name, &self.outdir)?;
        let symtab_out = self.artifact(ArtifactType::SymTabGoto);
        let goto = self.artifact(ArtifactType::Goto);

        let dry_run = self.session.args.dry_run;
        if dry_run || symtab_out.exists() {
            debug!(?symtab_out, "build link");
            self.session.link_goto_binary(&[symtab_out.to_path_buf()], goto)?;
        }

        // Create the project with the artifacts built by the compiler.
        let metadata_path = self.artifact(ArtifactType::Metadata);
        let metadata = if dry_run {
            dry_run_metadata(&self.crate_name)
        } else if metadata_path.exists() {
            self.metadata_with_function(from_json(metadata_path)?)
        } else {
            KaniMetadata::default()
        };

        Ok(Project {
            outdir: self.outdir,
            metadata: vec![metadata],
            artifacts: self
                .artifacts
                .into_values()
                .filter(|artifact| artifact.path.exists() || dry_run)
                .collect(),
        })
    }

    fn artifact(&self, typ: ArtifactType) -> &Path {
        &self.artifacts.get(&typ).unwrap().path
    }

    fn metadata_with_function(&self, mut metadata: KaniMetadata) -> KaniMetadata {
        if let Some(name) = &self.session.args.function {
            // --function is untranslated, create a mock harness
            metadata.proof_harnesses.push(mock_proof_harness(name, None, Some(&self.crate_name)));
        }
        metadata
    }
}

fn standalone_artifact(out_dir: &Path, crate_name: &String, typ: ArtifactType) -> Artifact {
    let mut path = out_dir.join(crate_name);
    let _ = path.set_extension(&typ);
    Artifact { harness_mangled: None, crate_name: crate_name.clone(), path, typ }
}

fn dry_run_metadata(crate_name: &str) -> KaniMetadata {
    KaniMetadata {
        crate_name: crate_name.to_string(),
        proof_harnesses: vec![mock_proof_harness("harness", None, Some(crate_name))],
        unsupported_features: vec![],
        test_harnesses: vec![],
    }
}
