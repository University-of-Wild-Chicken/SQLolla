import com.google.gson.Gson;
import com.google.gson.JsonObject;
import edu.mit.csail.sdg.alloy4.A4Reporter;
import edu.mit.csail.sdg.ast.Sig;
import edu.mit.csail.sdg.parser.CompModule;
import edu.mit.csail.sdg.parser.CompUtil;
import edu.mit.csail.sdg.translator.A4Options;
import edu.mit.csail.sdg.translator.A4Solution;
import edu.mit.csail.sdg.translator.A4Tuple;
import edu.mit.csail.sdg.translator.A4TupleSet;
import edu.mit.csail.sdg.translator.TranslateAlloyToKodkod;
import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import kodkod.engine.satlab.SATFactory;

/** Private JSON-lines Alloy API worker; Rust owns all SQL semantics and decisions. */
public final class AlloyBridge {
    /** Structured serialization only; keys are compiler-owned identifiers. */
    private static final Gson JSON = new Gson();
    /** Current private enumeration; Rust validates durable cursor and replay obligations. */
    private static A4Solution current;
    /** Parsed module belonging to the current enumeration. */
    private static CompModule currentModule;
    /** Command index belonging to the current enumeration. */
    private static int currentCommand;
    /** Zero-based solver ordinal, independent of any SQL row occurrence. */
    private static int currentOrdinal;

    /**
     * Receives bounded requests until EOF and flushes one response per request.
     * @param args unused; SQL and credentials are never command-line arguments
     * @throws Exception if the pipe cannot be read or written
     */
    public static void main(String[] args) throws Exception {
        BufferedReader input = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        while (true) {
            String line = readBounded(input);
            if (line == null) return;
            System.out.println(JSON.toJson(handle(line)));
            System.out.flush();
        }
    }

    /**
     * Bounds memory consumed while reading a complete generated model request.
     * @param input UTF-8 decoded private pipe
     * @return one line or null at EOF
     * @throws Exception on oversized input or pipe failure
     */
    private static String readBounded(BufferedReader input) throws Exception {
        StringBuilder line = new StringBuilder();
        int next;
        while ((next = input.read()) != -1 && next != '\n') {
            if (line.length() >= 4_194_304) throw new IllegalArgumentException("request too large");
            line.append((char) next);
        }
        return next == -1 && line.isEmpty() ? null : line.toString();
    }

    /**
     * Converts API failures into explicit engine errors, never UNSAT.
     * @param line complete internal JSON request
     * @return protocol response with solver outcome or diagnostic
     */
    private static Map<String, Object> handle(String line) {
        try {
            JsonObject request = JSON.fromJson(line, JsonObject.class);
            String action = request.has("action") ? request.get("action").getAsString() : "solve";
            if (action.equals("next")) return next();
            if (!action.equals("solve") && !action.equals("begin")) throw new IllegalArgumentException("unknown action");
            return solve(request.get("model").getAsString(), request.get("command").getAsInt(), action.equals("begin"));
        } catch (Exception error) {
            return Map.of("protocol", "sqlolla.alloy.v1", "status", "ERROR",
                "message", error.toString());
        }
    }

    /**
     * Executes one generated Alloy command with an explicitly selected portable solver.
     * @param model deterministic compiler output
     * @param command command index in that model
     * @param retain whether to retain this solution for on-demand enumeration
     * @return structured SAT/UNSAT and positional tuples
     * @throws Exception if parsing, translation or solving fails
     */
    private static Map<String, Object> solve(String model, int command, boolean retain) throws Exception {
        CompModule module = CompUtil.parseEverything_fromString(A4Reporter.NOP, model);
        if (command < 0 || command >= module.getAllCommands().size()) {
            throw new IllegalArgumentException("invalid command selector");
        }
        A4Options options = new A4Options();
        options.solver = SATFactory.get("sat4j");
        options.noOverflow = true;
        A4Solution solution = TranslateAlloyToKodkod.execute_command(A4Reporter.NOP,
            module.getAllReachableSigs(), module.getAllCommands().get(command), options);
        if (retain) {
            current = solution;
            currentModule = module;
            currentCommand = command;
            currentOrdinal = 0;
        }
        return response(solution, module, command, 0);
    }

    /**
     * Advance the retained API solution; errors and exhausted models remain distinct.
     * @return the next solution or the retained terminal UNSAT response
     * @throws Exception if enumeration was not initialized or the solver fails
     */
    private static Map<String, Object> next() throws Exception {
        if (current == null) throw new IllegalStateException("enumeration has not begun");
        if (current.satisfiable()) {
            current = current.next();
            currentOrdinal++;
        }
        return response(current, currentModule, currentCommand, currentOrdinal);
    }

    /**
     * Serialize command identity and one complete API solution without SQL interpretation.
     * @param solution current API solution
     * @param module parsed compiler-generated module
     * @param command selected command index
     * @param ordinal zero-based enumeration ordinal
     * @return complete solver metadata and satisfying tuples when available
     */
    private static Map<String, Object> response(A4Solution solution, CompModule module, int command, int ordinal) {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("protocol", "sqlolla.alloy.v1");
        result.put("status", solution.satisfiable() ? "SAT" : "UNSAT");
        result.put("command_kind", module.getAllCommands().get(command).check ? "check" : "run");
        result.put("command_label", module.getAllCommands().get(command).label);
        result.put("solver", "sat4j");
        result.put("bitwidth", solution.getBitwidth());
        result.put("ordinal", ordinal);
        result.put("java_version", System.getProperty("java.version"));
        if (solution.satisfiable()) result.put("relations", witness(solution, module));
        return result;
    }

    /**
     * Preserves signature identity and ordered relation tuple positions.
     * @param solution satisfying API solution
     * @param module generated model
     * @return signature and field relations; no semantic interpretation occurs here
     */
    private static Map<String, Object> witness(A4Solution solution, CompModule module) {
        Map<String, Object> relations = new LinkedHashMap<>();
        for (Sig signature : module.getAllReachableSigs()) {
            if (signature.builtin || !signature.label.startsWith("this/")) continue;
            relations.put(signature.label, tuples(solution.eval(signature)));
            for (Sig.Field field : signature.getFields()) {
                relations.put(signature.label + "." + field.label, tuples(solution.eval(field)));
            }
        }
        return relations;
    }

    /**
     * Reads n-ary tuples through typed API access instead of parsing display strings.
     * @param tuples evaluated signature or field
     * @return ordered atom lists
     */
    private static List<List<String>> tuples(A4TupleSet tuples) {
        List<List<String>> result = new ArrayList<>();
        for (A4Tuple tuple : tuples) {
            List<String> row = new ArrayList<>();
            for (int index = 0; index < tuple.arity(); index++) row.add(tuple.atom(index));
            result.add(row);
        }
        return result;
    }
}
