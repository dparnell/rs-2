// Decompile every function of an analysed program in address order (one
// Ghidra start for a whole small module instead of one per function).
// Script args: [0xMIN 0xMAX]
//@category rs2
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class DecompileAll extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        Address lo = a.length > 1 ? toAddr(a[0]) : null, hi = a.length > 1 ? toAddr(a[1]) : null;
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        for (Function f : currentProgram.getFunctionManager().getFunctions(true)) {
            Address e = f.getEntryPoint();
            if (lo != null && (e.compareTo(lo) < 0 || e.compareTo(hi) > 0)) continue;
            DecompileResults r = d.decompileFunction(f, 120, monitor);
            println("==== " + f.getName() + " @ " + e);
            println(r.getDecompiledFunction() != null ? r.getDecompiledFunction().getC() : r.getErrorMessage());
        }
    }
}
