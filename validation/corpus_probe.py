"""Probe unchanged industrial SQL grammar and binding without executing business DML."""
import json
from oracle import literal

PROBE_FUNCTION = """
CREATE FUNCTION pg_temp.corpus_probe(query text, identifier text, ddl boolean) RETURNS json LANGUAGE plpgsql AS $probe$
DECLARE state text; message text; detail text; params text[];
BEGIN
  IF ddl THEN
    BEGIN
      EXECUTE query;
      RAISE EXCEPTION USING ERRCODE='PZ001', MESSAGE='ddl_observed_and_rolled_back';
    EXCEPTION WHEN SQLSTATE 'PZ001' THEN
      RETURN json_build_object('id',identifier,'status','ddl_executed_then_rolled_back','sqlstate','00000');
    END;
  ELSE
    EXECUTE 'PREPARE corpus_probe_statement AS ' || query;
    SELECT parameter_types::text[] INTO params FROM pg_prepared_statements WHERE name='corpus_probe_statement';
    DEALLOCATE corpus_probe_statement;
    RETURN json_build_object('id',identifier,'status','prepared_not_executed','sqlstate','00000','inferred_parameter_types',params);
  END IF;
EXCEPTION WHEN OTHERS THEN
  GET STACKED DIAGNOSTICS state=RETURNED_SQLSTATE, message=MESSAGE_TEXT, detail=PG_EXCEPTION_DETAIL;
  RETURN json_build_object('id',identifier,'status','syntax_rejected'::text,'sqlstate',state,'message',message,'detail',detail);
END $probe$;
"""


def probe_records(database, records: dict, features: dict) -> dict:
    """Catch one PostgreSQL SQLSTATE per record; PREPARE is never execution evidence."""
    script = ["SET statement_timeout='2s'; SET lock_timeout='500ms'; SET standard_conforming_strings=on; SET timezone='UTC';", PROBE_FUNCTION]
    for identifier, record in records.items():
        ddl = features[identifier]["statement_prefix"] in {"CREATE", "DROP", "ALTER"}
        script.append(f"SELECT pg_temp.corpus_probe({literal(record['sql'])},{literal(identifier)},{literal(ddl)});")
    output = {}
    for line in database.execute("\n".join(script), timeout=120).splitlines():
        result = json.loads(line)
        if result["status"] == "syntax_rejected" and result["sqlstate"] != "42601":
            result["status"] = "context_or_analysis_error"
        output[result["id"]] = result
    if set(output) != set(records): raise ValueError("Incomplete PostgreSQL corpus probe")
    return output
