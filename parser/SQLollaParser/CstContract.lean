import SQLollaParser.Cst

/-! Exact production-shape contracts pinned to PostgreSQL16.15 and the reviewed dependency. -/
open SQLean.PostgresGrammar
namespace SQLollaParser

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts0 : List (Nat × String × List String) := [
  (1, "parse_toplevel", ["stmtmulti"]),
  (7, "stmtmulti", ["stmtmulti", "';'", "toplevel_stmt"]),
  (8, "stmtmulti", ["toplevel_stmt"]),
  (9, "toplevel_stmt", ["stmt"]),
  (10, "toplevel_stmt", ["TransactionStmtLegacy"]),
  (86, "stmt", ["DeleteStmt"]),
  (93, "stmt", ["DropStmt"]),
  (107, "stmt", ["InsertStmt"]),
  (125, "stmt", ["SelectStmt"]),
  (129, "stmt", ["UpdateStmt"]),
  (134, "stmt", ["ViewStmt"]),
  (135, "stmt", []),
  (142, "opt_drop_behavior", ["CASCADE"]),
  (143, "opt_drop_behavior", ["RESTRICT"]),
  (144, "opt_drop_behavior", []),
  (532, "opt_column_list", ["'('", "columnList", "')'"]),
  (533, "opt_column_list", []),
  (534, "columnList", ["columnElem"]),
  (535, "columnList", ["columnList", "','", "columnElem"]),
  (891, "DropStmt", ["DROP", "object_type_any_name", "IF_P", "EXISTS", "any_name_list", "opt_drop_behavior"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts1 : List (Nat × String × List String) := [
  (892, "DropStmt", ["DROP", "object_type_any_name", "any_name_list", "opt_drop_behavior"]),
  (905, "object_type_any_name", ["VIEW"]),
  (932, "any_name_list", ["any_name"]),
  (933, "any_name_list", ["any_name_list", "','", "any_name"]),
  (1079, "index_params", ["index_elem"]),
  (1080, "index_params", ["index_params", "','", "index_elem"]),
  (1081, "index_elem_options", ["opt_collate", "opt_qualified_name", "opt_asc_desc", "opt_nulls_order"]),
  (1083, "index_elem", ["ColId", "index_elem_options"]),
  (1092, "opt_asc_desc", ["ASC"]),
  (1093, "opt_asc_desc", ["DESC"]),
  (1094, "opt_asc_desc", []),
  (1095, "opt_nulls_order", ["NULLS_LA", "FIRST_P"]),
  (1096, "opt_nulls_order", ["NULLS_LA", "LAST_P"]),
  (1097, "opt_nulls_order", []),
  (1459, "ViewStmt", ["CREATE", "OptTemp", "VIEW", "qualified_name", "opt_column_list", "opt_reloptions", "AS", "SelectStmt", "opt_check_option"]),
  (1549, "opt_name_list", ["'('", "name_list", "')'"]),
  (1550, "opt_name_list", []),
  (1573, "PreparableStmt", ["SelectStmt"]),
  (1574, "PreparableStmt", ["InsertStmt"]),
  (1575, "PreparableStmt", ["UpdateStmt"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts2 : List (Nat × String × List String) := [
  (1576, "PreparableStmt", ["DeleteStmt"]),
  (1587, "InsertStmt", ["opt_with_clause", "INSERT", "INTO", "insert_target", "insert_rest", "opt_on_conflict", "returning_clause"]),
  (1588, "insert_target", ["qualified_name"]),
  (1589, "insert_target", ["qualified_name", "AS", "ColId"]),
  (1590, "insert_rest", ["SelectStmt"]),
  (1592, "insert_rest", ["'('", "insert_column_list", "')'", "SelectStmt"]),
  (1594, "insert_rest", ["DEFAULT", "VALUES"]),
  (1597, "insert_column_list", ["insert_column_item"]),
  (1598, "insert_column_list", ["insert_column_list", "','", "insert_column_item"]),
  (1599, "insert_column_item", ["ColId", "opt_indirection"]),
  (1600, "opt_on_conflict", ["ON", "CONFLICT", "opt_conf_expr", "DO", "UPDATE", "SET", "set_clause_list", "where_clause"]),
  (1601, "opt_on_conflict", ["ON", "CONFLICT", "opt_conf_expr", "DO", "NOTHING"]),
  (1602, "opt_on_conflict", []),
  (1603, "opt_conf_expr", ["'('", "index_params", "')'", "where_clause"]),
  (1604, "opt_conf_expr", ["ON", "CONSTRAINT", "name"]),
  (1605, "opt_conf_expr", []),
  (1606, "returning_clause", ["RETURNING", "target_list"]),
  (1607, "returning_clause", []),
  (1608, "DeleteStmt", ["opt_with_clause", "DELETE_P", "FROM", "relation_expr_opt_alias", "using_clause", "where_or_current_clause", "returning_clause"]),
  (1609, "using_clause", ["USING", "from_list"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts3 : List (Nat × String × List String) := [
  (1610, "using_clause", []),
  (1624, "opt_nowait_or_skip", ["NOWAIT"]),
  (1625, "opt_nowait_or_skip", ["SKIP", "LOCKED"]),
  (1626, "opt_nowait_or_skip", []),
  (1627, "UpdateStmt", ["opt_with_clause", "UPDATE", "relation_expr_opt_alias", "SET", "set_clause_list", "from_clause", "where_or_current_clause", "returning_clause"]),
  (1628, "set_clause_list", ["set_clause"]),
  (1629, "set_clause_list", ["set_clause_list", "','", "set_clause"]),
  (1630, "set_clause", ["set_target", "'='", "a_expr"]),
  (1632, "set_target", ["ColId", "opt_indirection"]),
  (1664, "SelectStmt", ["select_no_parens"]),
  (1665, "SelectStmt", ["select_with_parens"]),
  (1666, "select_with_parens", ["'('", "select_no_parens", "')'"]),
  (1667, "select_with_parens", ["'('", "select_with_parens", "')'"]),
  (1668, "select_no_parens", ["simple_select"]),
  (1669, "select_no_parens", ["select_clause", "sort_clause"]),
  (1670, "select_no_parens", ["select_clause", "opt_sort_clause", "for_locking_clause", "opt_select_limit"]),
  (1671, "select_no_parens", ["select_clause", "opt_sort_clause", "select_limit", "opt_for_locking_clause"]),
  (1672, "select_no_parens", ["with_clause", "select_clause"]),
  (1673, "select_no_parens", ["with_clause", "select_clause", "sort_clause"]),
  (1674, "select_no_parens", ["with_clause", "select_clause", "opt_sort_clause", "for_locking_clause", "opt_select_limit"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts4 : List (Nat × String × List String) := [
  (1675, "select_no_parens", ["with_clause", "select_clause", "opt_sort_clause", "select_limit", "opt_for_locking_clause"]),
  (1676, "select_clause", ["simple_select"]),
  (1677, "select_clause", ["select_with_parens"]),
  (1678, "simple_select", ["SELECT", "opt_all_clause", "opt_target_list", "into_clause", "from_clause", "where_clause", "group_clause", "having_clause", "window_clause"]),
  (1682, "simple_select", ["select_clause", "UNION", "set_quantifier", "select_clause"]),
  (1683, "simple_select", ["select_clause", "INTERSECT", "set_quantifier", "select_clause"]),
  (1684, "simple_select", ["select_clause", "EXCEPT", "set_quantifier", "select_clause"]),
  (1685, "with_clause", ["WITH", "cte_list"]),
  (1686, "with_clause", ["WITH_LA", "cte_list"]),
  (1687, "with_clause", ["WITH", "RECURSIVE", "cte_list"]),
  (1688, "cte_list", ["common_table_expr"]),
  (1689, "cte_list", ["cte_list", "','", "common_table_expr"]),
  (1690, "common_table_expr", ["name", "opt_name_list", "AS", "opt_materialized", "'('", "PreparableStmt", "')'", "opt_search_clause", "opt_cycle_clause"]),
  (1691, "opt_materialized", ["MATERIALIZED"]),
  (1692, "opt_materialized", ["NOT", "MATERIALIZED"]),
  (1693, "opt_materialized", []),
  (1700, "opt_with_clause", ["with_clause"]),
  (1701, "opt_with_clause", []),
  (1715, "set_quantifier", ["ALL"]),
  (1716, "set_quantifier", ["DISTINCT"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts5 : List (Nat × String × List String) := [
  (1717, "set_quantifier", []),
  (1720, "opt_all_clause", ["ALL"]),
  (1721, "opt_all_clause", []),
  (1724, "opt_sort_clause", ["sort_clause"]),
  (1725, "opt_sort_clause", []),
  (1726, "sort_clause", ["ORDER", "BY", "sortby_list"]),
  (1727, "sortby_list", ["sortby"]),
  (1728, "sortby_list", ["sortby_list", "','", "sortby"]),
  (1730, "sortby", ["a_expr", "opt_asc_desc", "opt_nulls_order"]),
  (1731, "select_limit", ["limit_clause", "offset_clause"]),
  (1732, "select_limit", ["offset_clause", "limit_clause"]),
  (1733, "select_limit", ["limit_clause"]),
  (1734, "select_limit", ["offset_clause"]),
  (1735, "opt_select_limit", ["select_limit"]),
  (1736, "opt_select_limit", []),
  (1737, "limit_clause", ["LIMIT", "select_limit_value"]),
  (1743, "offset_clause", ["OFFSET", "select_offset_value"]),
  (1745, "select_limit_value", ["a_expr"]),
  (1747, "select_offset_value", ["a_expr"]),
  (1757, "group_clause", ["GROUP_P", "BY", "set_quantifier", "group_by_list"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts6 : List (Nat × String × List String) := [
  (1758, "group_clause", []),
  (1759, "group_by_list", ["group_by_item"]),
  (1760, "group_by_list", ["group_by_list", "','", "group_by_item"]),
  (1761, "group_by_item", ["a_expr"]),
  (1770, "having_clause", ["HAVING", "a_expr"]),
  (1771, "having_clause", []),
  (1772, "for_locking_clause", ["for_locking_items"]),
  (1774, "opt_for_locking_clause", ["for_locking_clause"]),
  (1775, "opt_for_locking_clause", []),
  (1776, "for_locking_items", ["for_locking_item"]),
  (1777, "for_locking_items", ["for_locking_items", "for_locking_item"]),
  (1778, "for_locking_item", ["for_locking_strength", "locked_rels_list", "opt_nowait_or_skip"]),
  (1779, "for_locking_strength", ["FOR", "UPDATE"]),
  (1780, "for_locking_strength", ["FOR", "NO", "KEY", "UPDATE"]),
  (1781, "for_locking_strength", ["FOR", "SHARE"]),
  (1782, "for_locking_strength", ["FOR", "KEY", "SHARE"]),
  (1783, "locked_rels_list", ["OF", "qualified_name_list"]),
  (1784, "locked_rels_list", []),
  (1785, "values_clause", ["VALUES", "'('", "expr_list", "')'"]),
  (1786, "values_clause", ["values_clause", "','", "'('", "expr_list", "')'"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts7 : List (Nat × String × List String) := [
  (1787, "from_clause", ["FROM", "from_list"]),
  (1788, "from_clause", []),
  (1789, "from_list", ["table_ref"]),
  (1790, "from_list", ["from_list", "','", "table_ref"]),
  (1791, "table_ref", ["relation_expr", "opt_alias_clause"]),
  (1797, "table_ref", ["select_with_parens", "opt_alias_clause"]),
  (1798, "table_ref", ["LATERAL_P", "select_with_parens", "opt_alias_clause"]),
  (1799, "table_ref", ["joined_table"]),
  (1801, "joined_table", ["'('", "joined_table", "')'"]),
  (1802, "joined_table", ["table_ref", "CROSS", "JOIN", "table_ref"]),
  (1803, "joined_table", ["table_ref", "join_type", "JOIN", "table_ref", "join_qual"]),
  (1804, "joined_table", ["table_ref", "JOIN", "table_ref", "join_qual"]),
  (1805, "joined_table", ["table_ref", "NATURAL", "join_type", "JOIN", "table_ref"]),
  (1806, "joined_table", ["table_ref", "NATURAL", "JOIN", "table_ref"]),
  (1807, "alias_clause", ["AS", "ColId", "'('", "name_list", "')'"]),
  (1808, "alias_clause", ["AS", "ColId"]),
  (1809, "alias_clause", ["ColId", "'('", "name_list", "')'"]),
  (1810, "alias_clause", ["ColId"]),
  (1811, "opt_alias_clause", ["alias_clause"]),
  (1812, "opt_alias_clause", [])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts8 : List (Nat × String × List String) := [
  (1820, "join_type", ["FULL", "opt_outer"]),
  (1821, "join_type", ["LEFT", "opt_outer"]),
  (1822, "join_type", ["RIGHT", "opt_outer"]),
  (1823, "join_type", ["INNER_P"]),
  (1826, "join_qual", ["USING", "'('", "name_list", "')'", "opt_alias_clause_for_join_using"]),
  (1827, "join_qual", ["ON", "a_expr"]),
  (1828, "relation_expr", ["qualified_name"]),
  (1835, "relation_expr_opt_alias", ["relation_expr"]),
  (1836, "relation_expr_opt_alias", ["relation_expr", "ColId"]),
  (1837, "relation_expr_opt_alias", ["relation_expr", "AS", "ColId"]),
  (1850, "where_clause", ["WHERE", "a_expr"]),
  (1851, "where_clause", []),
  (1852, "where_or_current_clause", ["WHERE", "a_expr"]),
  (1854, "where_or_current_clause", []),
  (1877, "Typename", ["SimpleTypename", "opt_array_bounds"]),
  (1883, "opt_array_bounds", ["opt_array_bounds", "'['", "']'"]),
  (1885, "opt_array_bounds", []),
  (1886, "SimpleTypename", ["GenericType"]),
  (1888, "SimpleTypename", ["Bit"]),
  (1897, "GenericType", ["type_function_name", "opt_type_modifiers"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts9 : List (Nat × String × List String) := [
  (1914, "Bit", ["BitWithLength"]),
  (1915, "Bit", ["BitWithoutLength"]),
  (1918, "BitWithLength", ["BIT", "opt_varying", "'('", "expr_list", "')'"]),
  (1919, "BitWithoutLength", ["BIT", "opt_varying"]),
  (1932, "opt_varying", ["VARYING"]),
  (1933, "opt_varying", []),
  (1958, "a_expr", ["c_expr"]),
  (1959, "a_expr", ["a_expr", "TYPECAST", "Typename"]),
  (1964, "a_expr", ["a_expr", "'+'", "a_expr"]),
  (1965, "a_expr", ["a_expr", "'-'", "a_expr"]),
  (1966, "a_expr", ["a_expr", "'*'", "a_expr"]),
  (1967, "a_expr", ["a_expr", "'/'", "a_expr"]),
  (1970, "a_expr", ["a_expr", "'<'", "a_expr"]),
  (1971, "a_expr", ["a_expr", "'>'", "a_expr"]),
  (1972, "a_expr", ["a_expr", "'='", "a_expr"]),
  (1973, "a_expr", ["a_expr", "LESS_EQUALS", "a_expr"]),
  (1974, "a_expr", ["a_expr", "GREATER_EQUALS", "a_expr"]),
  (1975, "a_expr", ["a_expr", "NOT_EQUALS", "a_expr"]),
  (1976, "a_expr", ["a_expr", "qual_Op", "a_expr"]),
  (1978, "a_expr", ["a_expr", "AND", "a_expr"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts10 : List (Nat × String × List String) := [
  (1979, "a_expr", ["a_expr", "OR", "a_expr"]),
  (1980, "a_expr", ["NOT", "a_expr"]),
  (1994, "a_expr", ["a_expr", "IS", "NULL_P"]),
  (1996, "a_expr", ["a_expr", "IS", "NOT", "NULL_P"]),
  (2011, "a_expr", ["a_expr", "IN_P", "in_expr"]),
  (2012, "a_expr", ["a_expr", "NOT_LA", "IN_P", "in_expr"]),
  (2014, "a_expr", ["a_expr", "subquery_Op", "sub_type", "'('", "a_expr", "')'"]),
  (2024, "a_expr", ["DEFAULT"]),
  (2050, "c_expr", ["'('", "a_expr", "')'", "opt_indirection"]),
  (2051, "c_expr", ["case_expr"]),
  (2052, "c_expr", ["func_expr"]),
  (2055, "c_expr", ["EXISTS", "select_with_parens"]),
  (2056, "c_expr", ["ARRAY", "select_with_parens"]),
  (2057, "c_expr", ["ARRAY", "array_expr"]),
  (2061, "func_application", ["func_name", "'('", "')'"]),
  (2062, "func_application", ["func_name", "'('", "func_arg_list", "opt_sort_clause", "')'"]),
  (2068, "func_expr", ["func_application", "within_group_clause", "filter_clause", "over_clause"]),
  (2070, "func_expr", ["func_expr_common_subexpr"]),
  (2078, "func_expr_common_subexpr", ["CURRENT_TIMESTAMP"]),
  (2105, "func_expr_common_subexpr", ["NULLIF", "'('", "a_expr", "','", "a_expr", "')'"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts11 : List (Nat × String × List String) := [
  (2106, "func_expr_common_subexpr", ["COALESCE", "'('", "expr_list", "')'"]),
  (2161, "over_clause", ["OVER", "window_specification"]),
  (2163, "over_clause", []),
  (2164, "window_specification", ["'('", "opt_existing_window_name", "opt_partition_clause", "opt_sort_clause", "opt_frame_clause", "')'"]),
  (2165, "opt_existing_window_name", ["ColId"]),
  (2166, "opt_existing_window_name", []),
  (2167, "opt_partition_clause", ["PARTITION", "BY", "expr_list"]),
  (2168, "opt_partition_clause", []),
  (2218, "expr_list", ["a_expr"]),
  (2219, "expr_list", ["expr_list", "','", "a_expr"]),
  (2220, "func_arg_list", ["func_arg_expr"]),
  (2221, "func_arg_list", ["func_arg_list", "','", "func_arg_expr"]),
  (2222, "func_arg_expr", ["a_expr"]),
  (2229, "array_expr", ["'['", "expr_list", "']'"]),
  (2230, "array_expr", ["'['", "array_expr_list", "']'"]),
  (2231, "array_expr", ["'['", "']'"]),
  (2232, "array_expr_list", ["array_expr"]),
  (2233, "array_expr_list", ["array_expr_list", "','", "array_expr"]),
  (2258, "in_expr", ["select_with_parens"]),
  (2259, "in_expr", ["'('", "expr_list", "')'"])]

/-- Pin complete grammar alternatives used by the lowering dispatch; no semantic actions execute. -/
def productionContracts12 : List (Nat × String × List String) := [
  (2260, "case_expr", ["CASE", "case_arg", "when_clause_list", "case_default", "END_P"]),
  (2261, "when_clause_list", ["when_clause"]),
  (2262, "when_clause_list", ["when_clause_list", "when_clause"]),
  (2263, "when_clause", ["WHEN", "a_expr", "THEN", "a_expr"]),
  (2264, "case_default", ["ELSE", "a_expr"]),
  (2265, "case_default", []),
  (2266, "case_arg", ["a_expr"]),
  (2267, "case_arg", []),
  (2315, "opt_target_list", ["target_list"]),
  (2316, "opt_target_list", []),
  (2317, "target_list", ["target_el"]),
  (2318, "target_list", ["target_list", "','", "target_el"]),
  (2319, "target_el", ["a_expr", "AS", "ColLabel"]),
  (2320, "target_el", ["a_expr", "BareColLabel"]),
  (2321, "target_el", ["a_expr"]),
  (2322, "target_el", ["'*'"]),
  (2323, "qualified_name_list", ["qualified_name"]),
  (2324, "qualified_name_list", ["qualified_name_list", "','", "qualified_name"]),
  (2327, "name_list", ["name"]),
  (2328, "name_list", ["name_list", "','", "name"])]

/-- Pin qualified wildcard references and every accepted indirection item shape. -/
def productionContracts13 : List (Nat × String × List String) := [
  (1679, "simple_select", ["SELECT", "distinct_clause", "target_list", "into_clause", "from_clause", "where_clause", "group_clause", "having_clause", "window_clause"]),
  (1718, "distinct_clause", ["DISTINCT"]),
  (1719, "distinct_clause", ["DISTINCT", "ON", "'('", "expr_list", "')'"]),
  (2047, "c_expr", ["columnref"]),
  (2053, "c_expr", ["select_with_parens"]),
  (2269, "columnref", ["ColId", "indirection"]),
  (2270, "indirection_el", ["'.'", "attr_name"]),
  (2271, "indirection_el", ["'.'", "'*'"]),
  (2276, "indirection", ["indirection_el"]),
  (2277, "indirection", ["indirection", "indirection_el"])]

/-- Combine the maintained dispatch contracts in stable production order. -/
def productionContracts : List (Nat × String × List String) :=
  productionContracts0 ++ productionContracts1 ++ productionContracts2 ++ productionContracts3 ++ productionContracts4 ++ productionContracts5 ++ productionContracts6 ++ productionContracts7 ++ productionContracts8 ++ productionContracts9 ++ productionContracts10 ++ productionContracts11 ++ productionContracts12 ++ productionContracts13

/-- Reject grammar-table drift before lowering any certified syntax tree. -/
def productionContractValid : Bool :=
  productionContracts.all fun (index, lhs, rhs) =>
    match Generated.grammar.productions[index]? with
    | none => false
    | some definition =>
        grammarName (.nonterminal definition.lhs) == lhs && definition.rhs.map grammarName == rhs

end SQLollaParser
