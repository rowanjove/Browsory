"""Read-only rejection check with Python's independent, non-SQLCipher engine."""
import hashlib
import json
from pathlib import Path
import sqlite3
import sys


def file_hash(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def main():
    root = Path(sys.argv[1]).resolve(strict=True)
    if (root / ".browsory-synthetic-fixture").read_bytes() != b"synthetic-v1":
        raise ValueError("Synthetic fixture marker required")
    database = (root / sys.argv[2]).resolve(strict=True)
    if root not in database.parents or not database.is_file():
        raise ValueError("Database must be inside the synthetic fixture")
    output = root / "ordinary-sqlite-report.json"
    if output.exists():
        raise FileExistsError("Refuse to overwrite existing evidence")
    with sqlite3.connect(":memory:") as probe:
        if probe.execute("PRAGMA cipher_version").fetchone() is not None:
            raise RuntimeError("Expected ordinary SQLite, not SQLCipher")
    before = file_hash(database)
    code = None
    error_text = None
    connection = None
    try:
        connection = sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)
        connection.execute("SELECT count(*) FROM sqlite_schema").fetchone()
    except sqlite3.DatabaseError as error:
        code = getattr(error, "sqlite_errorcode", None)
        error_text = str(error)
        # sqlite_errorcode was added in Python 3.11. Older runtimes must match
        # SQLite's exact NOTADB message; unrelated I/O/locking failures fail.
        if code != 26 and not (
            code is None and error_text == "file is not a database"
        ):
            raise
    else:
        raise RuntimeError("Ordinary SQLite unexpectedly read the archive")
    finally:
        if connection is not None:
            connection.close()
    if file_hash(database) != before:
        raise RuntimeError("Read-only check changed the fixture")
    report = {
        "format_version": 1,
        "status": "passed",
        "engine": "Python stdlib ordinary SQLite",
        "sqlite_version": sqlite3.sqlite_version,
        "cipher_version_available": False,
        "database_sha256": before,
        "sqlite_errorcode": code,
        "error_message": error_text,
        "rejection_check": "error_code" if code is not None else "exact_legacy_error_message",
        "database_unchanged": True,
    }
    with output.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(report, handle, indent=2)
        handle.write("\n")
    print("Ordinary SQLite rejected the encrypted fixture; evidence: " + str(output))


if __name__ == "__main__":
    main()
