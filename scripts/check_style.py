"""Reject overlong authored Rust/Java/Python functions without examining build caches."""
from __future__ import annotations

import ast
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
TOKEN = re.compile(r'/\*.*?\*/|//[^\n]*|r(#+)?".*?"\1|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\\n])\'', re.S)
RUST = re.compile(r'(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)')
JAVA = re.compile(r'(?m)^\s*(?:public|private|protected)\s+(?:static\s+)?(?:final\s+)?[\w<>?, \[\]]+\s+(\w+)\s*\(')


def erase_literals(source: str) -> str:
    """Keep line locations while removing braces inside comments and literals."""
    return TOKEN.sub(lambda match: ''.join('\n' if c == '\n' else ' ' for c in match[0]), source)


def brace_ranges(source: str, pattern: re.Pattern) -> list[tuple[str, int, int]]:
    """Measure complete formatted function declarations including their signatures."""
    clean = erase_literals(source)
    ranges = []
    for match in pattern.finditer(clean):
        start = clean.find('{', match.end())
        if start < 0:
            continue
        depth = 1
        index = start + 1
        while index < len(clean) and depth:
            depth += (clean[index] == '{') - (clean[index] == '}')
            index += 1
        first = clean.count('\n', 0, match.start(1)) + 1
        last = clean.count('\n', 0, index) + 1
        ranges.append((match[1], first, last - first + 1))
    return ranges


def function_ranges(path: Path) -> list[tuple[str, int, int]]:
    """Use Python's parser for Python and lexical brace ranges for Rust/Java."""
    source = path.read_text(encoding='utf-8-sig')
    if path.suffix == '.py':
        return [(node.name, node.lineno, node.end_lineno - node.lineno + 1)
                for node in ast.walk(ast.parse(source)) if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))]
    return brace_ranges(source, RUST if path.suffix == '.rs' else JAVA)


def main() -> int:
    """Check maintained implementation and validation code, not historical probes."""
    failures = []
    paths = [path for folder in ('crates', 'bridge', 'validation', 'scripts', 'parser')
             for path in (ROOT / folder).rglob('*') if path.suffix in ('.rs', '.java', '.py')
             and not any(part.startswith('.') or part in ('target', 'classes', 'results') for part in path.relative_to(ROOT).parts)]
    for path in paths:
        for name, line, length in function_ranges(path):
            if length > 30:
                failures.append(f'{path.relative_to(ROOT)}:{line}: {name} has {length} lines')
    print('\n'.join(failures) if failures else f'Style gate passed for {len(paths)} source files.')
    return bool(failures)


if __name__ == '__main__':
    sys.exit(main())
