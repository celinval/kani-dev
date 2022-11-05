// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

trait Id {
    fn id(&self) -> u64;
}

trait Name {
    fn name(&self) -> &str;
}

struct Complex {
    name: String,
}

impl Id for Complex {
    fn id(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.name.hash(&mut hasher);
        hasher.finish()
    }
}

impl Name for Complex {
    fn name(&self) -> &str {
        &self.name
    }
}

fn from_str(name: &str) -> u64 {
    if name.is_empty() { 0 } else { 1 }
}

fn from_complex(name: &str) -> u64 {
    Complex { name: name.to_string() }.id()
}

fn from(func: &dyn Fn(&str) -> u64, name: &str) -> u64 {
    func(name)
}

#[kani::proof]
#[kani::unwind(3)]
fn check_empty() {
    let name = "ACD10";
    let id = Some(name).map(|name| from(&from_str, name));
    assert_eq!(id, Some(1));
}

#[kani::proof]
#[kani::unwind(6)]
fn check_name() {
    let name = "ACD10";
    let id = Some(name).map(|name| from(&from_complex, name));
    assert_eq!(id, Some(0xACD10));
}
