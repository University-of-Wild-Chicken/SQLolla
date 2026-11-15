"""Kill intentional Alloy semantic mutants using independent PostgreSQL cardinalities."""
import json
import os
import re

from cases import all_cases
from run import CLI, ROOT, RESULTS, digest, write_json
from rejections import execute
from process_helpers import communicate

SEEDS = {"null_equality": "int32/self_equal/b01", "except_rank": "int32/except_all/l09r03",
         "outer_join": "int32/join_left/l03r00", "drop_occurrence": "int32/project_aliases/b09",
         "and_dominance": "truth/filter_not_and", "integer_order": "integer/lt",
         "whole_row_distinct": "multicolumn/distinct", "drop_filter": "int32/is_null/b02",
         "unknown_to_false": "truth/filter_not_and", "union_duplicate": "int32/union_all/l03r03"}


def model_metadata(model: str) -> tuple[str, str, str, str]:
    """Read compiler-owned root/value names and the exact existing finite scope."""
    root = re.findall(r"abstract sig (N\d+) ", model)[-1]
    truth = re.search(r"fun nullv\[a:V\]:one V \{ a=Null => (V\d+) else (V\d+) \}", model)
    if truth is None: raise ValueError("Compiler truth naming changed; review mutation harness")
    scope = next(line[len("run QuerySemantics "):] for line in model.splitlines() if line.startswith("run QuerySemantics "))
    return root, truth.group(1), truth.group(2), scope


def mutate(label: str, model: str) -> tuple[str, int]:
    """Apply one intentionally wrong semantic rule, requiring a concrete match."""
    root, true, false, _ = model_metadata(model)
    rules = {
        "null_equality": (r"(fun eqv[^\n]+\) => )Null", rf"\g<1>{true}"),
        "except_rank": (r" >= ", " > "),
        "outer_join": (r" and no p:", " and some p:"),
        "drop_occurrence": (rf"{root}R0\.alive = [^\n]+", f"{root}R0.alive = ((some none) => {true} else {false})"),
        "and_dominance": (rf"(fun andv[^\n]+a={false}) or (b={false})", r"\1 and \2"),
        "integer_order": (r"fact \{ Ordering\.lt = [^\n]+", "fact { Ordering.lt = none->none }"),
        "whole_row_distinct": (r" and p\.c1=N\d+R\d+\.c1", ""),
        "drop_filter": (r"(N\d+R\d+\.alive) = [^\n]*nullv[^\n]*", rf"\g<1> = {true}"),
        "unknown_to_false": (r"(fun andv[^\n]+a=Null or b=Null\) => )Null", rf"\g<1>{false}"),
        "union_duplicate": (rf"{root}R0\.alive = [^\n]+", f"{root}R0.alive = ((some none) => {true} else {false})")}
    pattern, replacement = rules[label]
    changed, count = re.subn(pattern, replacement, model)
    if count == 0 or changed == model: raise ValueError(f"Mutation {label} matched nothing")
    return changed, count


def differing_count(model: str, expected: int, true_column: int | None = None) -> str:
    """Ask whether the complete fixed-input model can produce a wrong row count."""
    root, true, _, scope = model_metadata(model)
    predicate = f"r.alive={true}" + (f" and r.c{true_column}={true}" if true_column is not None else "")
    return model + f"assert OracleCardinality {{ QuerySemantics implies #{{r:{root} | {predicate}}} = {expected} }}\ncheck OracleCardinality {scope}\n"


def solve_models(models: list[str], command_index: int = 1) -> list[dict]:
    """Use the real Java17 Alloy API bridge on original and mutated generated models."""
    default_java = str(ROOT / "m0/alloy/.tools/jdk-17.0.21+9/bin/java.exe") if os.name == "nt" else "java"
    java = os.environ.get("SQLolla_JAVA", default_java)
    jar = os.environ.get("SQLolla_ALLOY_JAR", str(ROOT.parent / "dependencies/org.alloytools.alloy.dist.jar"))
    classpath = os.pathsep.join([str(ROOT / "bridge/classes"), str(jar)])
    command = [str(java), "-Xmx512m", "-Dfile.encoding=UTF-8", "-Djava.awt.headless=true", "-cp", classpath, "AlloyBridge"]
    wire = "".join(json.dumps({"model": model, "command": command_index}) + "\n" for model in models)
    output = communicate(command, wire, timeout=120, cwd=ROOT)
    return [json.loads(line) for line in output.splitlines()]


def inputs() -> tuple[list[str], list[dict], str]:
    """Compile fixed seed databases while retaining PostgreSQL-derived expected counts."""
    cases = {case["request"]["id"]: case for case in all_cases()}
    oracle = {row["id"]: row for row in map(json.loads, (RESULTS / "postgres.jsonl").read_text(encoding="utf-8").splitlines())}
    requests = [dict(cases[identifier]["request"], mode="compile", include_model=True) for identifier in SEEDS.values()]
    compiled = execute(requests)
    models, metadata = [], []
    directory = RESULTS / "mutants"
    directory.mkdir(exist_ok=True)
    for (label, identifier), response in zip(SEEDS.items(), compiled):
        if response.get("status") != "ok": raise ValueError(f"Seed compile failed: {identifier}: {response}")
        original = response["model"]
        mutant, replacements = mutate(label, original)
        (directory / f"{label}-original.als").write_text(original, encoding="utf-8")
        (directory / f"{label}-mutant.als").write_text(mutant, encoding="utf-8")
        column = 2 if label == "integer_order" else None
        count = sum(row[2] is True for row in oracle[identifier]["rows"]) if column is not None else len(oracle[identifier]["rows"])
        models.extend([differing_count(original, count, column), differing_count(mutant, count, column)])
        metadata.append({"mutation": label, "id": identifier, "expected_count": count, "true_column": column, "replacements": replacements})
    return models, metadata, compiled[0]["model"]


def insufficient_scope(model: str) -> dict:
    """Reject an inconsistent concrete scope instead of treating UNSAT as a clean query."""
    if "one sig N0R0 extends N0" not in model: raise ValueError("Scope mutation has no required input slot")
    mutant = model + "fact ForcedInsufficientCapacity { no N0 }\n"
    original, changed = solve_models([model, mutant], command_index=0)
    rejected = original["status"] == "SAT" and changed["status"] == "UNSAT"
    return {"original": original["status"], "mutant": changed["status"], "mutation": "required singleton input versus explicit no-input capacity constraint", "rejected_as_inconsistent_execution": rejected,
            "meaning": "UNSAT from the base concrete-execution model is an engine/encoding failure, not property safety."}


def main() -> None:
    """Require original equivalence UNSAT and mutant disagreement SAT for every seed."""
    models, metadata, scope_model = inputs()
    responses = solve_models(models)
    if len(responses) != len(models): raise ValueError("Incomplete mutation solver output")
    for index, item in enumerate(metadata):
        item["original_difference"] = responses[index * 2]["status"]
        item["mutant_difference"] = responses[index * 2 + 1]["status"]
        item["killed"] = item["original_difference"] == "UNSAT" and item["mutant_difference"] == "SAT"
    scope_result = insufficient_scope(scope_model)
    passed = all(item["killed"] for item in metadata) and scope_result["rejected_as_inconsistent_execution"]
    report = {"status": "passed" if passed else "failed", "mutants": metadata,
              "cli_sha256": digest(CLI.read_bytes()),
              "insufficient_scope": scope_result,
              "scope": "Ten generated-model semantic mutations plus inconsistent-scope detection; cardinality-sensitive seeds, not universal mutation completeness."}
    write_json(RESULTS / "mutations.json", report)
    print(json.dumps(report))
    if not passed: raise SystemExit(1)


if __name__ == "__main__":
    main()
