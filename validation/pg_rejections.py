"""Verify disputed type-resolution/coercion boundaries against PostgreSQL itself."""
import json
from pg_runtime import TemporaryPostgres
from run import RESULTS, write_json

INVALID_SQL = {
    "values_local_unknown": "(VALUES (NULL)) UNION (VALUES (1))",
    "nested_unknown": "(SELECT NULL UNION SELECT NULL) UNION SELECT 1",
    "unicode_integer_whitespace": "SELECT 1 = '\u00a01\u00a0'",
    "unicode_boolean_whitespace": "SELECT TRUE = '\u00a0true\u00a0'",
}


def main() -> None:
    """Require actual server rejection for each independently specified invalid query."""
    outcomes = []
    with TemporaryPostgres() as database:
        for label, sql in INVALID_SQL.items():
            try:
                output = database.execute(sql + ";")
                outcomes.append({"id": label, "rejected": False, "output": output})
            except RuntimeError as error:
                outcomes.append({"id": label, "rejected": True, "diagnostic": str(error)})
        directory = database.directory
    passed = all(item["rejected"] for item in outcomes)
    report = {"status": "passed" if passed else "failed", "outcomes": outcomes, "cluster_removed": not directory.exists()}
    write_json(RESULTS / "pg-rejections.json", report)
    print(json.dumps(report, ensure_ascii=True))
    if not passed: raise SystemExit(1)


if __name__ == "__main__":
    main()
