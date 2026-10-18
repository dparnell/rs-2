// Headless helper: for each entry offset, disassemble the reachable code and
// report the operand of the first RETF found after the entry (the number of
// argument bytes a 16-bit _Pascal function pops).
// Script args: SEG:OFF=TAG ... or @FILE with one such token per line.
//@category rs2
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Instruction;

public class RetfScan extends GhidraScript {
    @Override
    public void run() throws Exception {
        java.util.List<String> args = new java.util.ArrayList<>();
        for (String a : getScriptArgs()) {
            if (a.startsWith("@")) {
                for (String l : java.nio.file.Files.readAllLines(java.nio.file.Path.of(a.substring(1)))) {
                    for (String t : l.trim().split("\\s+")) if (!t.isEmpty()) args.add(t);
                }
            } else {
                args.add(a);
            }
        }
        for (String arg : args) {
            String[] parts = arg.split("=");
            Address a = toAddr(parts[0]);
            disassemble(a);
            Instruction i = getInstructionAt(a);
            String result = "?";
            int n = 0;
            while (i != null && n < 2000) {
                String m = i.getMnemonicString();
                if (m.equals("RETF")) {
                    result = i.getNumOperands() > 0 ? i.getDefaultOperandRepresentation(0) : "0";
                    break;
                }
                Instruction next = i.getNext();
                if (next == null) {
                    Address na = i.getMaxAddress().add(1);
                    disassemble(na);
                    next = getInstructionAt(na);
                }
                i = next;
                n++;
            }
            println("RETF " + parts[1] + " " + result);
        }
    }
}
