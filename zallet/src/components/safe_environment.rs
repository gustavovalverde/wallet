//! Safe environment variable source for config-rs
//!
//! This module provides a temporary workaround for Unicode and race condition
//! issues in config-rs environment variable handling. It can be removed once
//! the upstream config-rs accepts our `source_os()` method.
//! https://github.com/rust-cli/config-rs/pull/683

use std::env;

use config::{ConfigError, Map, Source, Value, ValueKind};

/// A safe environment source that prevents Unicode panics and race conditions
///
/// This is a temporary implementation until config-rs upstream accepts
/// the `source_os()` method for handling `OsString` environment variables.
#[derive(Clone, Debug)]
pub struct SafeEnvironment {
    /// Prefix to filter environment variables
    prefix: String,
    /// Separator for nested keys  
    separator: String,
    /// Prefix separator
    prefix_separator: String,
    /// Whether to try parsing values as primitives
    try_parsing: bool,
    /// List separator for comma-separated values
    list_separator: Option<String>,
    /// Keys that should be parsed as lists
    list_parse_keys: Option<Vec<String>>,
    /// Pre-filtered environment variables (safe, no race conditions)
    filtered_env: Map<String, String>,
}

impl SafeEnvironment {
    /// Create a new SafeEnvironment with the given prefix
    ///
    /// This safely filters environment variables without Unicode panics or race conditions
    pub fn with_prefix_and_filter<F>(prefix: &str, filter_fn: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> bool,
    {
        let mut filtered_env = Map::new();

        // Single atomic snapshot - no race condition
        for (key, value) in env::vars_os() {
            // Safe Unicode conversion - no panic
            if let Some(key_str) = key.to_str() {
                if let Some(stripped) = key_str.strip_prefix(&format!("{prefix}_")) {
                    if filter_fn(stripped) {
                        // Only convert to String after we know it's valid Unicode
                        if let Some(value_str) = value.to_str() {
                            filtered_env.insert(key_str.to_owned(), value_str.to_owned());
                        }
                        // Non-Unicode values are silently ignored
                    }
                }
            }
            // Non-Unicode keys are silently ignored
        }

        Ok(Self {
            prefix: prefix.to_owned(),
            separator: "__".to_owned(),
            prefix_separator: "_".to_owned(),
            try_parsing: true,
            list_separator: Some(",".to_owned()),
            list_parse_keys: None,
            filtered_env,
        })
    }

    /// Set the separator for nested keys (default: "__")
    pub fn separator(mut self, separator: &str) -> Self {
        self.separator = separator.to_owned();
        self
    }

    /// Set the prefix separator (default: "_")  
    pub fn prefix_separator(mut self, prefix_separator: &str) -> Self {
        self.prefix_separator = prefix_separator.to_owned();
        self
    }

    /// Enable/disable parsing of primitive types (default: true)
    pub fn try_parsing(mut self, try_parsing: bool) -> Self {
        self.try_parsing = try_parsing;
        self
    }

    /// Set the list separator for comma-separated values (default: ",")
    pub fn list_separator(mut self, separator: &str) -> Self {
        self.list_separator = Some(separator.to_owned());
        self
    }

    /// Add a key that should be parsed as a list
    pub fn with_list_parse_key(mut self, key: &str) -> Self {
        let keys = self.list_parse_keys.get_or_insert_with(Vec::new);
        keys.push(key.to_owned());
        self
    }
}

impl Source for SafeEnvironment {
    fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
        Box::new((*self).clone())
    }

    fn collect(&self) -> Result<Map<String, Value>, ConfigError> {
        let mut m = Map::new();
        let uri: String = "the environment".into();

        for (key, value) in &self.filtered_env {
            let mut processed_key = key.to_lowercase();

            // Remove prefix
            let prefix_pattern = format!("{}{}", self.prefix.to_lowercase(), self.prefix_separator);
            if processed_key.starts_with(&prefix_pattern) {
                processed_key = processed_key[prefix_pattern.len()..].to_string();
            }

            // Replace separator with dots
            if !self.separator.is_empty() {
                processed_key = processed_key.replace(&self.separator, ".");
            }

            let processed_value = if self.try_parsing {
                // Try parsing as primitives first
                if let Ok(parsed) = value.to_lowercase().parse::<bool>() {
                    ValueKind::Boolean(parsed)
                } else if let Ok(parsed) = value.parse::<i64>() {
                    ValueKind::I64(parsed)
                } else if let Ok(parsed) = value.parse::<f64>() {
                    ValueKind::Float(parsed)
                } else if let Some(separator) = &self.list_separator {
                    // Handle list parsing
                    if let Some(keys) = &self.list_parse_keys {
                        if keys.contains(&processed_key) {
                            let v: Vec<Value> = value
                                .split(separator)
                                .map(|s| Value::new(Some(&uri), ValueKind::String(s.to_owned())))
                                .collect();
                            ValueKind::Array(v)
                        } else {
                            ValueKind::String(value.clone())
                        }
                    } else {
                        ValueKind::String(value.clone())
                    }
                } else {
                    ValueKind::String(value.clone())
                }
            } else {
                ValueKind::String(value.clone())
            };

            m.insert(processed_key, Value::new(Some(&uri), processed_value));
        }

        Ok(m)
    }
}
