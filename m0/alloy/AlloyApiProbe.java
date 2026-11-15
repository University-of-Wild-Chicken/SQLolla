import com.google.gson.GsonBuilder;
import edu.mit.csail.sdg.alloy4.A4Reporter;
import edu.mit.csail.sdg.ast.Command;
import edu.mit.csail.sdg.ast.Sig;
import edu.mit.csail.sdg.parser.CompModule;
import edu.mit.csail.sdg.parser.CompUtil;
import edu.mit.csail.sdg.translator.A4Options;
import edu.mit.csail.sdg.translator.A4Solution;
import edu.mit.csail.sdg.translator.A4Tuple;
import edu.mit.csail.sdg.translator.A4TupleSet;
import edu.mit.csail.sdg.translator.TranslateAlloyToKodkod;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import kodkod.engine.satlab.SATFactory;

/** Disposable M0 probe of the pinned Alloy API; not a SQL compiler or service. */
public final class AlloyApiProbe {
    /** A fixed authored model and its expected satisfiability. */
    private record Probe(String id, String model, boolean noOverflow, boolean expected) {}

    /** Records the API's post-CNF callback without writing protocol output. */
    private static final class Metrics extends A4Reporter {
        /** CNF measurements collected in callback order. */
        private final List<Map<String, Integer>> calls = new ArrayList<>();

        /**
         * Captures generated CNF sizes reported by the Alloy translator.
         * @param prefix temporal prefix length reported by Alloy
         * @param primary primary variable count
         * @param total total variable count
         * @param clauses clause count
         */
        @Override
        public void solve(int prefix, int primary, int total, int clauses) {
            calls.add(Map.of("prefixLength", prefix, "primaryVariables", primary,
                "totalVariables", total, "clauses", clauses));
        }
    }

    /**
     * Runs fixed feasibility cases and exits unsuccessfully on any mismatch.
     * @param args unused; models are deliberately fixed in this probe
     * @throws Exception if the API fails or a case disagrees with its expectation
     */
    public static void main(String[] args) throws Exception {
        SATFactory solver = SATFactory.get("sat4j");
        Map<String, Object> output = environment(solver);
        List<Map<String, Object>> outcomes = new ArrayList<>();
        boolean passed = true;
        for (Probe probe : probes()) {
            Map<String, Object> outcome = execute(probe, solver);
            outcomes.add(outcome);
            passed &= Boolean.TRUE.equals(outcome.get("matchesExpected"));
        }
        Map<String, Object> enumeration = enumerate(solver);
        passed &= Boolean.TRUE.equals(enumeration.get("matchesExpected"));
        output.put("cases", outcomes);
        output.put("enumeration", enumeration);
        output.put("allMatchedExpected", passed);
        System.out.println(new GsonBuilder().setPrettyPrinting().create().toJson(output));
        if (!passed) throw new IllegalStateException("M0 probe expectation mismatch");
    }

    /**
     * Defines SAT, nontrivial UNSAT, and arithmetic-boundary probes.
     * @return immutable fixed probe definitions
     */
    private static List<Probe> probes() {
        String sat = "sig Row { value: one Int }\n"
            + "run { one Row and all r: Row | r.value = 3 } for 3 but 4 Int";
        String unsat = "sig Row { key: one Key } sig Key {}\n"
            + "run { all disj a,b: Row | a.key != b.key } "
            + "for exactly 3 Row, exactly 2 Key";
        return List.of(new Probe("relational_sat", sat, true, true),
            new Probe("pigeonhole_unsat", unsat, true, false),
            new Probe("overflow_wrap_enabled", arithmetic(7, -8, 4), false, true),
            new Probe("overflow_rejected", arithmetic(7, -8, 4), true, false),
            new Probe("in_range_four_bits", arithmetic(6, 7, 4), true, true),
            new Probe("widened_five_bits", arithmetic(7, 8, 5), true, true));
    }

    /**
     * Constructs a tiny integer model to expose the configured overflow policy.
     * @param value input integer constrained in the model
     * @param result required result of adding one
     * @param bits signed Alloy integer bitwidth
     * @return a complete generated Alloy module
     */
    private static String arithmetic(int value, int result, int bits) {
        return "open util/integer\none sig N { value: one Int }\n"
            + "run { N.value = " + value + " and add[N.value,1] = " + result
            + " } for " + bits + " Int";
    }

    /**
     * Invokes the public Alloy parser and translation APIs for one fixed model.
     * @param probe fixed model and expected result
     * @param solver configured SAT factory
     * @return structured result with options, metrics and any witness
     * @throws Exception if parsing, translation or solving fails
     */
    private static Map<String, Object> execute(Probe probe, SATFactory solver) throws Exception {
        Metrics metrics = new Metrics();
        CompModule module = CompUtil.parseEverything_fromString(metrics, probe.model());
        Command command = module.getAllCommands().get(0);
        A4Options options = new A4Options();
        options.solver = solver;
        options.noOverflow = probe.noOverflow();
        long started = System.nanoTime();
        A4Solution solution = TranslateAlloyToKodkod.execute_command(
            metrics, module.getAllReachableSigs(), command, options);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("id", probe.id());
        result.put("model", probe.model());
        result.put("expectedSat", probe.expected());
        result.put("satisfiable", solution.satisfiable());
        result.put("matchesExpected", solution.satisfiable() == probe.expected());
        result.put("noOverflow", options.noOverflow);
        result.put("bitwidth", solution.getBitwidth());
        result.put("elapsedMilliseconds", (System.nanoTime() - started) / 1_000_000);
        result.put("cnfCallbacks", metrics.calls);
        if (solution.satisfiable()) result.put("witness", witness(solution, module));
        return result;
    }

    /**
     * Extracts user signature and field tuples through A4Solution.eval.
     * @param solution satisfiable API solution
     * @param module parsed module whose local signatures should be extracted
     * @return signature and field names mapped to positional tuples
     */
    private static Map<String, Object> witness(A4Solution solution, CompModule module) {
        Map<String, Object> result = new LinkedHashMap<>();
        for (Sig signature : module.getAllReachableSigs()) {
            if (signature.builtin || !signature.label.startsWith("this/")) continue;
            result.put(signature.label, tuples(solution.eval(signature)));
            for (Sig.Field field : signature.getFields()) {
                result.put(signature.label + "." + field.label, tuples(solution.eval(field)));
            }
        }
        return result;
    }

    /**
     * Serializes typed Alloy tuple positions without parsing display strings.
     * @param set API tuple set to inspect
     * @return tuple atoms in the API's positional order
     */
    private static List<List<String>> tuples(A4TupleSet set) {
        List<List<String>> result = new ArrayList<>();
        for (A4Tuple tuple : set) {
            List<String> atoms = new ArrayList<>();
            for (int index = 0; index < tuple.arity(); index++) atoms.add(tuple.atom(index));
            result.add(atoms);
        }
        return result;
    }

    /**
     * Enumerates a fixed finite model and retains every tuple position.
     * @param solver incremental SAT backend
     * @return bounded enumeration evidence and expectation checks
     * @throws Exception if parsing, solving or next-instance enumeration fails
     */
    private static Map<String, Object> enumerate(SATFactory solver) throws Exception {
        String model = "one sig Key {} one sig Val {}\n"
            + "one sig Record { link: Key -> Val, flag: one Int }\n"
            + "run { some Record.link and (Record.flag=1 or Record.flag=2) } for 3 Int";
        Metrics metrics = new Metrics();
        CompModule module = CompUtil.parseEverything_fromString(metrics, model);
        A4Options options = new A4Options();
        options.solver = solver;
        options.noOverflow = true;
        A4Solution solution = TranslateAlloyToKodkod.execute_command(
            metrics, module.getAllReachableSigs(), module.getAllCommands().get(0), options);
        List<Map<String, Object>> instances = new ArrayList<>();
        List<String> flags = new ArrayList<>();
        boolean orderedTernary = true;
        while (solution.satisfiable() && instances.size() < 3) {
            instances.add(witness(solution, module));
            flags.add(tuples(solution.eval(field(module, "flag"))).get(0).get(1));
            orderedTernary &= ternaryPositions(solution, field(module, "link"));
            solution = solution.next();
        }
        Map<String, Object> result = enumerationResult(instances, flags,
            !solution.satisfiable(), orderedTernary);
        result.put("model", model);
        result.put("cnfCallbacks", metrics.calls);
        return result;
    }

    /**
     * Resolves a field in this authored model without parsing printed tuples.
     * @param module fixed model containing unique local field names
     * @param label exact field name
     * @return matching API field
     * @throws IllegalArgumentException if the fixed probe definition is inconsistent
     */
    private static Sig.Field field(CompModule module, String label) {
        for (Sig signature : module.getAllReachableSigs()) {
            for (Sig.Field field : signature.getFields()) {
                if (field.label.equals(label)) return field;
            }
        }
        throw new IllegalArgumentException("Missing probe field: " + label);
    }

    /**
     * Checks owner/key/value positions in an arity-three relation.
     * @param solution current satisfiable instance
     * @param field relation declared as Record -> Key -> Val
     * @return whether exactly one tuple preserves all three declared positions
     */
    private static boolean ternaryPositions(A4Solution solution, Sig.Field field) {
        List<List<String>> rows = tuples(solution.eval(field));
        if (rows.size() != 1 || rows.get(0).size() != 3) return false;
        List<String> tuple = rows.get(0);
        return tuple.get(0).startsWith("Record$") && tuple.get(1).startsWith("Key$")
            && tuple.get(2).startsWith("Val$");
    }

    /**
     * Records finite exhaustion separately from an enumeration cap.
     * @param instances decoded ordered tuple evidence
     * @param flags observed typed integer values distinguishing instances
     * @param exhausted whether the next solution was UNSAT
     * @param orderedTernary whether positional checks passed in every instance
     * @return enumeration metadata and the complete fixed-model expectation
     */
    private static Map<String, Object> enumerationResult(List<Map<String, Object>> instances,
            List<String> flags, boolean exhausted, boolean orderedTernary) {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("instances", instances);
        result.put("instanceCount", instances.size());
        result.put("semanticFlagValues", flags);
        result.put("nextCalls", instances.size());
        result.put("cap", 3);
        result.put("exhausted", exhausted);
        result.put("capped", !exhausted && instances.size() == 3);
        result.put("orderedTernaryPositionsPreserved", orderedTernary);
        result.put("matchesExpected", exhausted && instances.size() == 2
            && flags.containsAll(List.of("1", "2")) && orderedTernary);
        return result;
    }

    /**
     * Identifies the actual JVM and selected backend used for this probe run.
     * @param solver selected SAT factory
     * @return runtime facts and the observed default overflow option
     */
    private static Map<String, Object> environment(SATFactory solver) {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("javaVersion", System.getProperty("java.version"));
        result.put("javaRuntimeVersion", System.getProperty("java.runtime.version"));
        result.put("javaVendor", System.getProperty("java.vendor"));
        result.put("osName", System.getProperty("os.name"));
        result.put("osArch", System.getProperty("os.arch"));
        result.put("solver", solver.id());
        result.put("solverType", solver.type());
        result.put("solverPresent", solver.isPresent());
        result.put("defaultNoOverflow", new A4Options().noOverflow);
        return result;
    }
}
