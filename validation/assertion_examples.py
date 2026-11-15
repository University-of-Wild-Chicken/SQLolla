"""Regenerate inspectable SQL/IR/Alloy pairs with actual assertion checks and one missing-filter mutant."""
import json
from itertools import product

from cases import fixture, small_bags, table, typed_cell
from oracle import oracle_script, rows_to_bag, typed_bag
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, ROOT, digest, write_json
from symbolic import property_violated

TENANT = "SELECT tenant_id FROM orders WHERE tenant_id = $1"
JOIN = "SELECT l.x AS left_x, r.x AS right_x FROM ledger AS l LEFT JOIN rates AS r ON l.x = r.x"
GROUP = "SELECT tenant_id, COUNT(*) AS n FROM orders GROUP BY tenant_id HAVING COUNT(*) > 1 ORDER BY tenant_id"


def request(label: str, sql: str, names: list[str], property: dict, bound: int = 2) -> dict:
    """Declare exact finite inputs and a policy without constraining the model to satisfy it."""
    columns = [("tenant_id" if name == "orders" else "x", "int32") for name in names]
    sources = {name: table([column], []) for name, column in zip(names, columns)}
    result = fixture(label, sql, sources, [], "assertion_example")["request"]
    result.update(mode="analyze", include_model=True, database={}, property=property,
                  bounds={"public." + name: bound for name in names},
                  domain=[typed_cell(value, "int32") for value in [None, 1, 2]])
    if "$1" in sql: result["parameters"] = {"1": typed_cell(1, "int32")}
    return result


def requests() -> list[tuple[dict, str]]:
    """Pair three meaningful properties with a negative control that must expose a tenant leak."""
    tenant = {"kind": "only_value", "column": 0, "value": typed_cell(1, "int32")}
    required_match = {"kind": "no_value", "column": 1, "value": typed_cell(None, "int32")}
    return [(request("tenant-filter", TENANT, ["orders"], tenant), "NO_COUNTEREXAMPLE_WITHIN_BOUNDS"),
            (request("tenant-missing-filter", "SELECT tenant_id FROM orders", ["orders"], tenant), "COUNTEREXAMPLE"),
            (request("left-join", JOIN, ["ledger", "rates"], required_match), "COUNTEREXAMPLE"),
            (request("group-count", GROUP, ["orders"], {"kind": "max_rows", "max": 1}, 3), "NO_COUNTEREXAMPLE_WITHIN_BOUNDS")]


def main() -> None:
    """Publish only examples whose base model is satisfiable and actual Alloy check verdict matches."""
    cases = requests()
    responses = index_responses([row for row, _ in cases], execute([row for row, _ in cases]))
    postgres = check_postgres(cases, responses)
    examples = []
    for submitted, expected in cases:
        response = responses[submitted["id"]]
        if response.get("analysis") != expected or response.get("assumptions") != "SATISFIABLE": raise ValueError(response)
        if response.get("property_check", {}).get("command") != "check": raise ValueError("Not an Alloy assertion check")
        model = response.pop("model")
        name = submitted["id"] + ".als"
        write_model(name, submitted, response, model)
        examples.append({"request": submitted, "model_file": name, "result": response, "postgres": postgres[submitted["id"]]})
    write_json(ROOT / "examples/translation-pairs.json", {"cli_sha256": digest(CLI.read_bytes()), "examples": examples})
    print(json.dumps({"status": "passed", "checks": len(examples), "verdicts": {e["request"]["id"]: e["result"]["analysis"] for e in examples}}))


def write_model(name: str, request: dict, response: dict, model: str) -> None:
    """Keep the exact generated model body with commented SQL, policy and finite scope context."""
    metadata = {key: request[key] for key in ("sql", "property", "bounds", "domain")}
    metadata["profile"] = response["profile"]
    header = "// SQLolla example: " + json.dumps(metadata, ensure_ascii=True) + "\n"
    (ROOT / "examples" / name).write_text(header + model, encoding="utf-8")


def concrete(request: dict, database: dict, schema: list, identifier: str) -> dict:
    """Translate explicit small input bags into independent PostgreSQL tables without rewriting SQL."""
    sources = {value["name"]: table([(c["name"], c["ty"]) for c in value["columns"]], database[key])
               for key, value in request["catalog"]["tables"].items()}
    result = fixture(identifier, request["sql"], sources, [c["ty"] for c in schema], "assertion_oracle")
    result["request"]["parameters"] = request.get("parameters", {})
    return result


def worlds(request: dict, schema: list) -> list[dict]:
    """Enumerate the entire declared finite bag domain, independently of Rust or Alloy enumeration."""
    keys = list(request["catalog"]["tables"])
    domains = [small_bags([None, 1, 2], request["bounds"][key]) for key in keys]
    return [concrete(request, {key: [[value] for value in bag] for key, bag in zip(keys, bags)}, schema,
                     request["id"] + "/" + str(index)) for index, bags in enumerate(product(*domains))]


def check_postgres(cases: list, responses: dict) -> dict:
    """Require matching SAT existence over every input bag and independently replay both witnesses."""
    generated = {request["id"]: worlds(request, responses[request["id"]]["schema"]) for request, _ in cases}
    all_cases = [case for values in generated.values() for case in values]
    with TemporaryPostgres() as database:
        observations = {row["id"]: row["rows"] for row in map(json.loads, database.execute(oracle_script(all_cases)).splitlines())}
        evidence = {}
        for request, expected in cases:
            rows = [observations[case["request"]["id"]] for case in generated[request["id"]]]
            violation = any(property_violated(request["property"], bag) for bag in rows)
            if violation != (expected == "COUNTEREXAMPLE"): raise ValueError("Expected property disagrees with PostgreSQL")
            replay = replay_witness(database, request, responses[request["id"]]) if violation else None
            evidence[request["id"]] = {"world_count": len(rows), "has_counterexample": violation,
                "observations_sha256": digest(json.dumps(rows, sort_keys=True).encode()), "witness_replay": replay}
        version, directory = database.execute("SELECT version();").strip(), database.directory
    for value in evidence.values(): value.update(version=version, cluster_removed=not directory.exists())
    return evidence


def replay_witness(database, request: dict, response: dict) -> dict:
    """Check solver witness domain membership before executing that exact database in PostgreSQL."""
    witness = response["instance"]
    raw = {}
    if set(witness["database"]) != set(request["bounds"]): raise ValueError("Witness catalog differs")
    for key, rows in witness["database"].items():
        if len(rows) > request["bounds"][key] or any(len(row) != 1 or row[0] not in request["domain"] for row in rows):
            raise ValueError("Witness input outside declared finite scope")
        raw[key] = [[None if row[0]["type"] == "null" else row[0]["value"]] for row in rows]
    case = concrete(request, raw, response["schema"], request["id"] + "/witness")
    pg = json.loads(database.execute(oracle_script([case])))
    if rows_to_bag(pg["rows"], case["oracle"]["output_types"]) != typed_bag(witness["output"]): raise ValueError("PostgreSQL witness replay differs")
    if not property_violated(request["property"], pg["rows"]): raise ValueError("Replayed witness does not violate property")
    return {"rows": pg["rows"], "matches": True}


if __name__ == "__main__":
    main()
