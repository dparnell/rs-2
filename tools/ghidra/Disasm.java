// Headless helper: print a disassembly listing.  Script args: START END
//@category rs2
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Instruction;

public class Disasm extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        Address start = toAddr(a[0]), end = toAddr(a[1]);
        Address p = start;
        while (p.compareTo(end) < 0) {
            Instruction i = getInstructionAt(p);
            if (i == null) { disassemble(p); i = getInstructionAt(p); }
            if (i == null) { println(p + ": db " + String.format("%02x", getByte(p) & 0xff)); p = p.add(1); continue; }
            println(p + ": " + i);
            p = i.getMaxAddress().add(1);
        }
    }
}
