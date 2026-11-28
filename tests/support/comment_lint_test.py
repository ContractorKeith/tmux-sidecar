import sys
from pathlib import Path
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
from comment_lint import comments, compare

rust = '''// gone
let url = "http://x"; let raw = r#"// not a comment"#; let c = '/';
/// doc
// SAFETY: required by clippy
// continued safety reason
unsafe { f() } // trailing
'''
assert [line for line, _ in comments('a.rs', rust)] == [1, 3, 6], comments('a.rs', rust)
python = '#!/usr/bin/env python3\n"""Doc."""\nx = "# no"  # yes\n'
assert [line for line, _ in comments('a.py', python)] == [2, 3]
assert [line for line, _ in comments('a.sh', '#!/bin/sh\n# yes\necho "#no"\n')] == [2]

assert compare({'a.rs': 2}, {'a.rs': 2}) == []
assert 'baseline allows 1' in compare({'a.rs': 2}, {'a.rs': 1})[0]
assert '--update' in compare({'a.rs': 1}, {'a.rs': 2})[0]
assert 'baseline allows 0' in compare({'new.rs': 1}, {})[0]
print('comment_lint_test: ok')
