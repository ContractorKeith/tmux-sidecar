import ast
import io
import re
import subprocess
import sys
import tokenize
from pathlib import Path

BASELINE = Path(__file__).with_name('comment-baseline.txt')
SUFFIXES = {'.rs', '.py', '.sh', '.yml', '.yaml'}


def rust_comments(src):
    lines, i, n = [], 0, len(src)
    while i < n:
        if src.startswith('//', i):
            j = src.find('\n', i)
            j = n if j < 0 else j
            alone = not src[src.rfind('\n', 0, i) + 1:i].strip()
            lines.append((src.count('\n', 0, i) + 1, src[i:j], alone))
            i = j
        elif src.startswith('/*', i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith('/*', j):
                    depth, j = depth + 1, j + 2
                elif src.startswith('*/', j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            first = src.count('\n', 0, i) + 1
            for k, text in enumerate(src[i:j].split('\n')):
                lines.append((first + k, text, False))
            i = j
        elif (m := re.match(r'b?r(#*)"', src[i:i + 260])) and not (i and (src[i - 1].isalnum() or src[i - 1] == '_')):
            end = '"' + m.group(1)
            i = src.index(end, i + len(m.group(0))) + len(end)
        elif src[i] == '"':
            j = i + 1
            while src[j] != '"':
                j += 2 if src[j] == '\\' else 1
            i = j + 1
        elif src[i] == "'" and (m := re.match(r"'(\\.[^']*|[^'\\])'", src[i:i + 12])):
            i += len(m.group(0))
        else:
            i += 1
    kept, safety_line = [], None
    for line, text, alone in lines:
        plain = text.strip()
        if plain.startswith('// SAFETY:') or (
                alone and safety_line == line - 1 and not plain.startswith(('///', '//!'))):
            safety_line = line
            continue
        safety_line = None
        kept.append((line, plain))
    return kept


def python_comments(src):
    found = [(t.start[0], t.string) for t in tokenize.generate_tokens(io.StringIO(src).readline)
             if t.type == tokenize.COMMENT and not (t.start[0] == 1 and t.string.startswith('#!'))]
    for node in ast.walk(ast.parse(src)):
        body = getattr(node, 'body', None)
        if isinstance(body, list) and body and isinstance(body[0], ast.Expr) \
                and isinstance(body[0].value, ast.Constant) and isinstance(body[0].value.value, str):
            found += [(body[0].lineno + k, text) for k, text in enumerate(ast.get_source_segment(src, body[0]).split('\n'))]
    return sorted(found)


def hash_comments(src):
    return [(k, line.strip()) for k, line in enumerate(src.split('\n'), 1)
            if line.lstrip().startswith('#') and not (k == 1 and line.startswith('#!'))]


def comments(path, src):
    suffix = Path(path).suffix
    if suffix == '.rs':
        return rust_comments(src)
    if suffix == '.py':
        return python_comments(src)
    return hash_comments(src)


def tracked(root):
    names = subprocess.run(['git', 'ls-files', '-z'], cwd=root, capture_output=True, check=True).stdout
    for name in names.decode().split('\0'):
        if name and Path(name).suffix in SUFFIXES and (root / name).is_file():
            yield name


def counts(root):
    found = {}
    for name in tracked(root):
        n = len(comments(name, (root / name).read_text()))
        if n:
            found[name] = n
    return found


def read_baseline(path):
    if not path.exists():
        return {}
    pairs = (line.split(None, 1) for line in path.read_text().splitlines() if line.strip())
    return {name.strip(): int(n) for n, name in pairs}


def write_baseline(path, values):
    path.write_text(''.join(f'{n} {name}\n' for name, n in sorted(values.items())))


def compare(current, baseline):
    errors = []
    for name in sorted(set(current) | set(baseline)):
        now, allowed = current.get(name, 0), baseline.get(name, 0)
        if now > allowed:
            errors.append(f'{name}: {now} comment lines, baseline allows {allowed}. '
                          'Delete them; record decisions in docs/adr/ (ADR-0003).')
        elif now < allowed:
            errors.append(f'{name}: {now} comment lines, baseline still says {allowed}. '
                          'Run python3 scripts/comment_lint.py --update to lower it.')
    return errors


def main(argv):
    root = Path(__file__).resolve().parent.parent
    current = counts(root)
    if '--list' in argv:
        for name in sorted(current):
            for line, text in comments(name, (root / name).read_text()):
                print(f'{name}:{line}: {text}')
        return 0
    baseline = read_baseline(BASELINE)
    errors = compare(current, baseline)
    if '--update' in argv:
        raised = [e for e in errors if 'baseline allows' in e]
        if raised:
            print('\n'.join(raised), file=sys.stderr)
            return 1
        write_baseline(BASELINE, current)
        return 0
    if errors:
        print('\n'.join(errors), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
