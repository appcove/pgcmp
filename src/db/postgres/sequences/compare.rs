use super::fetch::SequenceInfo;
use std::collections::HashMap;

/// A detailed difference found when comparing sequences
#[derive(Debug, Clone)]
pub struct SequenceDiff {
    pub schema_name: String,
    pub sequence_name: String,
    pub differences: Vec<String>,
}

impl SequenceDiff {
    pub fn is_empty(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two sequences and return detailed differences
pub fn compare_sequences(old: &SequenceInfo, new: &SequenceInfo) -> SequenceDiff {
    let mut differences = Vec::new();
    let full_name = format!("{}.{}", new.schema_name, new.sequence_name);

    // Check data type change
    if old.data_type != new.data_type {
        differences.push(format!(
            "ALTER SEQUENCE TYPE: {} — change data type from '{}' to '{}'",
            full_name, old.data_type, new.data_type
        ));
    }

    // Check start value change
    if old.start_value != new.start_value {
        differences.push(format!(
            "ALTER SEQUENCE START: {} — change START WITH from {} to {}",
            full_name, old.start_value, new.start_value
        ));
    }

    // Check min value change
    if old.min_value != new.min_value {
        differences.push(format!(
            "ALTER SEQUENCE MINVALUE: {} — change MINVALUE from {} to {}",
            full_name, old.min_value, new.min_value
        ));
    }

    // Check max value change
    if old.max_value != new.max_value {
        differences.push(format!(
            "ALTER SEQUENCE MAXVALUE: {} — change MAXVALUE from {} to {}",
            full_name, old.max_value, new.max_value
        ));
    }

    // Check increment change
    if old.increment_by != new.increment_by {
        differences.push(format!(
            "ALTER SEQUENCE INCREMENT: {} — change INCREMENT BY from {} to {}",
            full_name, old.increment_by, new.increment_by
        ));
    }

    // Check cycle change
    if old.cycle != new.cycle {
        if new.cycle {
            differences.push(format!(
                "ALTER SEQUENCE CYCLE: {} — add CYCLE (sequence will wrap around)",
                full_name
            ));
        } else {
            differences.push(format!(
                "ALTER SEQUENCE NO CYCLE: {} — remove CYCLE (sequence will error at limit)",
                full_name
            ));
        }
    }

    // Check cache change
    if old.cache_size != new.cache_size {
        differences.push(format!(
            "ALTER SEQUENCE CACHE: {} — change CACHE from {} to {}",
            full_name, old.cache_size, new.cache_size
        ));
    }

    SequenceDiff {
        schema_name: new.schema_name.clone(),
        sequence_name: new.sequence_name.clone(),
        differences,
    }
}

/// Compare lists of sequences and return all differences
pub fn compare_sequence_lists(old: &[SequenceInfo], new: &[SequenceInfo]) -> Vec<SequenceDiff> {
    let mut results = Vec::new();

    let old_map: HashMap<(&str, &str), &SequenceInfo> = old
        .iter()
        .map(|s| ((s.schema_name.as_str(), s.sequence_name.as_str()), s))
        .collect();

    let new_map: HashMap<(&str, &str), &SequenceInfo> = new
        .iter()
        .map(|s| ((s.schema_name.as_str(), s.sequence_name.as_str()), s))
        .collect();

    // Check for added sequences
    for seq in new {
        let key = (seq.schema_name.as_str(), seq.sequence_name.as_str());
        if !old_map.contains_key(&key) {
            let mut diff = SequenceDiff {
                schema_name: seq.schema_name.clone(),
                sequence_name: seq.sequence_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "CREATE SEQUENCE: {}.{} — new {} sequence starting at {}, increment by {}{}",
                seq.schema_name,
                seq.sequence_name,
                seq.data_type,
                seq.start_value,
                seq.increment_by,
                if seq.cycle { ", with CYCLE" } else { "" }
            ));
            results.push(diff);
        }
    }

    // Check for removed sequences
    for seq in old {
        let key = (seq.schema_name.as_str(), seq.sequence_name.as_str());
        if !new_map.contains_key(&key) {
            let mut diff = SequenceDiff {
                schema_name: seq.schema_name.clone(),
                sequence_name: seq.sequence_name.clone(),
                differences: Vec::new(),
            };
            diff.differences.push(format!(
                "DROP SEQUENCE: {}.{} — sequence no longer exists",
                seq.schema_name, seq.sequence_name
            ));
            results.push(diff);
        }
    }

    // Check for modified sequences
    for new_seq in new {
        let key = (new_seq.schema_name.as_str(), new_seq.sequence_name.as_str());
        if let Some(old_seq) = old_map.get(&key) {
            let diff = compare_sequences(old_seq, new_seq);
            if !diff.is_empty() {
                results.push(diff);
            }
        }
    }

    results
}
