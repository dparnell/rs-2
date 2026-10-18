// Headless helper: decompile functions at the given addresses.
// Usage (script args): ADDR[=NAME] ...   e.g. 0x55d80=EP2_32_Decompress
//@category rs2
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;

public class Decompile extends GhidraScript {
    @Override
    public void run() throws Exception {
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        for (String arg : getScriptArgs()) {
            String[] parts = arg.split("=");
            Address a = toAddr(parts[0]);
            disassemble(a);
            Function f = getFunctionAt(a);
            if (f == null) f = createFunction(a, parts.length > 1 ? parts[1] : null);
            if (f == null) { println("no function at " + a); continue; }
            if (parts.length > 1) f.setName(parts[1], SourceType.USER_DEFINED);
            DecompileResults r = d.decompileFunction(f, 120, monitor);
            println("==== " + f.getName() + " @ " + a);
            println(r.getDecompiledFunction() != null ? r.getDecompiledFunction().getC() : r.getErrorMessage());
        }
    }
}
