// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! This module defines the structure for a Kani project.

use crate::metadata::{from_json, mock_proof_harness};
use crate::session::KaniSession;
use crate::util::{crate_name, guess_rlib_name};
use anyhow::{Context, Result};
use kani_metadata::{ArtifactType, HarnessMetadata, KaniMetadata};
use std::path::{Path, PathBuf};

/// This structure represent the project information relevant for verification.
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
            .map(|crate_metadata| {
                crate_metadata.proof_harnesses.iter().chain(crate_metadata.test_harnesses.iter())
            })
            .flatten()
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

    /// Return the matching artifact for the given krate. If more than one artifact is found,
    /// this will return the first element.
    pub fn get_crate_artifact(&self, krate: &String, typ: ArtifactType) -> Option<&Artifact> {
        self.artifacts.iter().find(|artifact| artifact.has_type(typ) && artifact.krate == *krate)
    }

    pub fn get_crate_artifacts(&self, krate: &String, typ: ArtifactType) -> Vec<&Artifact> {
        self.artifacts
            .iter()
            .filter(|artifact| artifact.has_type(typ) && artifact.krate == *krate)
            .collect()
    }
}

// Information about a build artifact.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Artifact {
    /// The name of the harness that this artifact is relative to a harness, if that's the case.
    harness_mangled: Option<String>,
    /// The name of the crate that originated this artifact.
    krate: String,
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

impl Artifact {
    fn has_type(&self, typ: ArtifactType) -> bool {
        self.typ == typ
    }
}

fn standalone_artifact(out_dir: &Path, crate_name: &String, typ: ArtifactType) -> Artifact {
    let mut path = out_dir.join(crate_name);
    let _ = path.set_extension(&typ);
    Artifact { harness_mangled: None, krate: crate_name.clone(), path, typ }
}

pub fn standalone_project(input: &Path, session: &KaniSession) -> Result<Project> {
    let outdir = session.args.target_dir.clone().unwrap_or(
        input
            .canonicalize()?
            .parent()
            .context(format!("Invalid input file location: {input:?}"))?
            .to_owned(),
    );
    // Register artifacts that may be generated by the compiler / linker for future deletion.
    let krate = crate_name(&input);
    let artifacts = [
        ArtifactType::Metadata,
        ArtifactType::Goto,
        ArtifactType::SymTab,
        ArtifactType::SymTabGoto,
        ArtifactType::TypeMap,
        ArtifactType::VTableRestriction,
    ]
    .map(|typ| standalone_artifact(&outdir, &krate, typ));
    session.record_temporary_files(&artifacts.each_ref());

    let rlib_path = guess_rlib_name(&outdir.join(input.file_name().unwrap()));
    session.record_temporary_files(&[&rlib_path]);

    // Invoke the compiler to build artifact files.
    session.compile_single_rust_file(&input, &krate, &outdir)?;
    let symtab_out = artifacts[3].as_ref();
    if session.args.dry_run || symtab_out.exists() {
        session.link_goto_binary(&[artifacts[3].path.clone()], &artifacts[1].path)?;
    }

    // Create the project with the artifacts built by the compiler.
    // TODO: Clean this code. It's pretty ugly right now.
    let metadata = &artifacts[0];
    if metadata.as_ref().exists() {
        let mut crate_metadata: KaniMetadata = from_json(artifacts[0].as_ref())?;
        if let Some(name) = &session.args.function {
            // --function is untranslated, create a mock harness
            crate_metadata.proof_harnesses.push(mock_proof_harness(name, None, Some(&krate)));
        }

        Ok(Project {
            outdir,
            metadata: vec![crate_metadata],
            artifacts: artifacts
                .into_iter()
                .filter(|artifact| artifact.as_ref().exists())
                .collect(),
        })
    } else {
        // TODO: Dry-run
        // Compilation didn't produce any artifacts. This can happen if there is no harness in
        // the given crate.
        Ok(Project::default())
    }
}

pub fn cargo_project(session: &KaniSession) -> Result<Project> {
    let _outputs = session.cargo_build()?;
    // This should be done per crate not per project.
    //let linked_obj = outputs.outdir.join("cbmc-linked.out");
    //session.link_goto_binary(&goto_objs, &linked_obj)?;
    todo!("Link and translate to Project.")
}
