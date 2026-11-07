//! Line alignment and three-line unified hunks, including empty files and missing newlines.
use std::{collections::HashMap, path::Path};
fn lines<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(char, &'a str)> {
    let mut positions: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, line) in new.iter().enumerate() {
        positions.entry(line).or_default().push(index);
    }
    let (mut a, mut b) = (0, 0);
    let mut result = Vec::new();
    while a < old.len() || b < new.len() {
        if a < old.len() && b < new.len() && old[a] == new[b] {
            result.push((' ', old[a]));
            a += 1;
            b += 1;
            continue;
        }
        // Resynchronize at the nearest shared line, retaining unchanged interior lines.
        let mut next = (old.len(), new.len());
        let mut distance = usize::MAX;
        for (offset, line) in old[a..].iter().enumerate() {
            if offset >= distance {
                break;
            }
            if let Some(indices) = positions.get(line)
                && let Some(&index) = indices.get(indices.partition_point(|index| *index < b))
                && offset + index - b < distance
            {
                distance = offset + index - b;
                next = (a + offset, index);
            }
        }
        result.extend(old[a..next.0].iter().map(|line| ('-', *line)));
        result.extend(new[b..next.1].iter().map(|line| ('+', *line)));
        (a, b) = next;
    }
    result
}
pub(super) fn unified(path: &Path, before: &str, after: &str) -> String {
    let old: Vec<_> = before.split_inclusive('\n').collect();
    let new: Vec<_> = after.split_inclusive('\n').collect();
    let lines = lines(&old, &new);
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for (index, (marker, _)) in lines.iter().enumerate() {
        if *marker == ' ' {
            continue;
        }
        let start = index.saturating_sub(3);
        let end = (index + 4).min(lines.len());
        if let Some(last) = hunks.last_mut()
            && start <= last.1
        {
            last.1 = end;
        } else {
            hunks.push((start, end));
        }
    }
    let mut text = format!("--- {}\n+++ {}\n", path.display(), path.display());
    let (mut old_at, mut new_at, mut previous) = (0, 0, 0);
    for (start, end) in hunks {
        for (marker, _) in &lines[previous..start] {
            old_at += usize::from(*marker != '+');
            new_at += usize::from(*marker != '-');
        }
        let old_count = lines[start..end].iter().filter(|(m, _)| *m != '+').count();
        let new_count = lines[start..end].iter().filter(|(m, _)| *m != '-').count();
        text.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            old_at + usize::from(old_count > 0),
            old_count,
            new_at + usize::from(new_count > 0),
            new_count
        ));
        for (marker, line) in &lines[start..end] {
            text.push(*marker);
            text.push_str(line);
            if !line.ends_with('\n') {
                text.push_str("\n\\ No newline at end of file\n");
            }
        }
        old_at += old_count;
        new_at += new_count;
        previous = end;
    }
    text
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separated_edits_are_separate_hunks_with_unchanged_context() {
        let before: String = (0..30).map(|i| format!("line {i}\n")).collect();
        let after = before
            .replace("line 2\n", "changed 2\n")
            .replace("line 27\n", "changed 27\n");
        let diff = unified(Path::new("settings"), &before, &after);
        assert_eq!(
            diff.lines().filter(|line| line.starts_with("@@")).count(),
            2,
            "{diff}"
        );
        assert!(!diff.contains("-line 15"), "{diff}");
    }
}
