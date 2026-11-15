import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;

/** Non-production M0 probe for explicit UTF-8 pipes and process cancellation. */
public final class ProtocolProbe {
    /** Echoes one framed line, or waits so its parent can test cancellation. */
    public static void main(String[] args) throws Exception {
        var out = new PrintWriter(new OutputStreamWriter(System.out, StandardCharsets.UTF_8), true);
        var err = new PrintWriter(new OutputStreamWriter(System.err, StandardCharsets.UTF_8), true);
        if (args.length > 0 && args[0].equals("sleep")) {
            out.println("ready");
            Thread.sleep(60_000);
            return;
        }
        err.println(args.length > 1 ? args[1] : "diagnostic");
        var in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        out.println(in.readLine());
    }
}
