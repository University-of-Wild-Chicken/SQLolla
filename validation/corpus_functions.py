"""Observe immutable SQL function declarations without substituting corpus-specific behavior."""
FUNCTION_SQL = """SELECT json_build_object('schema',n.nspname,'name',p.proname,'body',p.prosrc,
 'language',l.lanname,'volatility',p.provolatile,'strict',p.proisstrict,'security_definer',p.prosecdef,
 'returns_set',p.proretset,'can_execute',has_function_privilege(p.oid,'EXECUTE'),
 'can_use_schema',has_schema_privilege(n.oid,'USAGE'),'argument_names',p.proargnames,'argument_modes',p.proargmodes,
 'defaults',p.pronargdefaults,'settings',p.proconfig,'return_type',format_type(p.prorettype,NULL),
 'argument_types',ARRAY(SELECT format_type(t,NULL) FROM unnest(p.proargtypes)WITH ORDINALITY a(t,i)ORDER BY i))
 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace JOIN pg_language l ON l.oid=p.prolang
 WHERE n.nspname='public' AND p.prokind='f' ORDER BY n.nspname,p.proname,p.oid;"""


def supported(function: dict) -> bool:
    """The declaration profile excludes effect/privilege/settings/default and overloaded signatures."""
    return (function["language"] == "sql" and function["volatility"] == "i" and not function["strict"]
            and not function["security_definer"] and not function["returns_set"] and not function["settings"]
            and function["defaults"] == 0 and function["argument_modes"] is None)


def declared(observations: list[dict], enums: dict, sql_type) -> dict | None:
    """Retain the exact inspected SQL body and types; Rust will independently parse and validate it."""
    definitions = []
    for function in observations:
        if not supported(function): continue
        if sum((other["schema"], other["name"]) == (function["schema"], function["name"]) for other in observations) != 1: continue
        names = function["argument_names"] or ["arg" + str(index + 1) for index in range(len(function["argument_types"]))]
        parameters = [{"name": name or "arg" + str(index + 1), "ty": sql_type(ty, enums), "nullable": True}
                      for index, (name, ty) in enumerate(zip(names, function["argument_types"], strict=True))]
        definitions.append({"profile": "sql_immutable_non_strict_invoker_v1", "schema": function["schema"], "name": function["name"],
                            "can_execute": function["can_execute"], "can_use_schema": function["can_use_schema"],
                            "parameters": parameters, "returns": sql_type(function["return_type"], enums), "body_sql": function["body"]})
    return {"search_path": ["public"], "definitions": definitions} if definitions else None
