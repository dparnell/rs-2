// Decompile functions of an analysed program by name or address, or list
// the callers of one, or disassemble a range.
// Script args: NAME|0xADDR|callers:NAME|dis:0xSTART:0xEND ...
//@category rs2
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.*;

public class DecompileNamed extends GhidraScript {
    private Address resolve(String n) {
        if (n.startsWith("0x")) return toAddr(n);
        for (Symbol s : currentProgram.getSymbolTable().getSymbols(n)) return s.getAddress();
        return null;
    }

    @Override
    public void run() throws Exception {
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        for (String arg : getScriptArgs()) {
            if (arg.startsWith("dis:")) {
                String[] r = arg.split(":");
                Address p = toAddr(r[1]), end = toAddr(r[2]);
                println("==== " + arg);
                while (p.compareTo(end) < 0) {
                    ghidra.program.model.listing.Instruction i = getInstructionAt(p);
                    if (i == null) { disassemble(p); i = getInstructionAt(p); }
                    if (i == null) { println(p + ": db " + String.format("%02x", getByte(p) & 0xff)); p = p.add(1); continue; }
                    Symbol l = getSymbolAt(p);
                    println(p + ": " + (l != null ? l.getName() + ": " : "") + i);
                    p = i.getMaxAddress().add(1);
                }
                continue;
            }
            boolean callers = arg.startsWith("callers:");
            String n = callers ? arg.substring(8) : arg;
            Address a = resolve(n);
            if (a == null) { println("==== " + n + ": not found"); continue; }
            if (callers) {
                println("==== callers of " + n);
                for (Reference r : getReferencesTo(a)) {
                    Function f = getFunctionContaining(r.getFromAddress());
                    println("  " + r.getFromAddress() + " " + (f == null ? "?" : f.getName()));
                }
                continue;
            }
            Function f = getFunctionAt(a);
            if (f == null) { disassemble(a); f = createFunction(a, null); }
            if (f == null) { println("==== " + n + ": no function at " + a); continue; }
            DecompileResults r = d.decompileFunction(f, 180, monitor);
            println("==== " + f.getName() + " @ " + a);
            println(r.getDecompiledFunction() != null ? r.getDecompiledFunction().getC() : r.getErrorMessage());
        }
    }
}
