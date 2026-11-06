//! Turns raw vault lines into plain view data. No HTTP or HTML in here.

pub mod activity;
pub mod clinical;
pub mod documents;
pub mod explorer;
pub mod labs;
pub mod observation;
pub mod overview;
pub mod value;
pub mod vitals;

use crate::canon::parse_json;
use crate::error::PhvError;
use crate::store::Vault;
use serde_json::Value;
use std::collections::HashMap;

/// Reads resources for one request and counts lines it could not read.
pub struct Records {
    vault: Vault,
    unreadable: HashMap<String, usize>,
}

impl Records {
    pub fn new(vault: Vault) -> Self {
        Self {
            vault,
            unreadable: HashMap::new(),
        }
    }

    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    /// Every readable resource of a type, in file order.
    pub fn of_type(&mut self, resource_type: &str) -> Result<Vec<Value>, PhvError> {
        let mut resources = Vec::new();
        let mut bad = 0;
        for line in self.vault.raw_lines(resource_type)? {
            let text = std::str::from_utf8(&line).ok();
            match text.and_then(|line| parse_resource(line, resource_type)) {
                Some(resource) => resources.push(resource),
                None => bad += 1,
            }
        }
        self.unreadable.insert(resource_type.to_string(), bad);
        Ok(resources)
    }

    /// Lines that could not be read, across every type loaded so far.
    pub fn unreadable(&self) -> usize {
        self.unreadable.values().sum()
    }
}

/// A line counts as readable only if it is valid JSON with the expected type and an `id`.
pub fn parse_resource(line: &str, resource_type: &str) -> Option<Value> {
    let resource = parse_json(line).ok()?;
    (resource["resourceType"] == resource_type && resource["id"].is_string()).then_some(resource)
}
