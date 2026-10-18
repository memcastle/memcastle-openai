#!/usr/bin/env python3
"""Enforce native line coverage for one source, excluding inline test modules."""

import argparse
from pathlib import Path


def coverage(module: str, report: Path) -> tuple[int, int]:
    source_dir = Path(__file__).resolve().parents[1] / "sources" / module / "src"
    lines: dict[tuple[Path, int], int] = {}
    file: Path | None = None
    test_start: dict[Path, int] = {}
    for record in report.read_text().splitlines():
        if record.startswith("SF:"):
            file = Path(record[3:]).resolve()
            if file.is_relative_to(source_dir) and file.suffix == ".rs" and file.exists():
                source_lines = file.read_text().splitlines()
                test_start[file] = next(
                    (
                        index
                        for index, line in enumerate(source_lines, 1)
                        if line.strip() == "#[cfg(test)]"
                        and index < len(source_lines)
                        and source_lines[index].strip() == "mod tests {"
                    ),
                    len(source_lines) + 1,
                )
        elif record.startswith("DA:") and file in test_start:
            number, hits, *_ = record[3:].split(",")
            number, hits = int(number), int(hits)
            if number < test_start[file]:
                lines[(file, number)] = max(lines.get((file, number), 0), hits)
        elif record == "end_of_record":
            file = None
    return sum(hits > 0 for hits in lines.values()), len(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("module", choices=("codex", "chatgpt"))
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    covered, total = coverage(args.module, args.report)
    percent = 100 * covered / total if total else 0
    print(f"{args.module}: {covered}/{total} production lines ({percent:.2f}%)")
    if not total or 100 * covered <= 80 * total:
        parser.exit(1, f"{args.module} source coverage must be strictly greater than 80%\n")


if __name__ == "__main__":
    main()
