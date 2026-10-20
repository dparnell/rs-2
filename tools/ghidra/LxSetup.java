// Pre-analysis script for tools/ghidra/lxload.sh: adds the other objects
// of an LX module laid out by `cargo run -p os2-exe --example lximage`,
// an import area (one RET per import) and the symbols.
// Script args: DIR (holding layout.txt, objN.bin, symbols.txt)
//@category rs2
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;

public class LxSetup extends GhidraScript {
    @Override
    public void run() throws Exception {
        String dir = getScriptArgs()[0];
        Memory mem = currentProgram.getMemory();
        Set<String> code16 = new HashSet<>();
        for (String line : Files.readAllLines(Paths.get(dir, "layout.txt"))) {
            String[] f = line.trim().split("\\s+");
            Address base = toAddr(f[1]);
            if (f[0].equals("imports")) {
                int size = Integer.decode(f[2]);
                if (size == 0) continue;
                byte[] rets = new byte[size];
                Arrays.fill(rets, (byte) 0xc3);
                MemoryBlock b = mem.createInitializedBlock("imports", base,
                        new ByteArrayInputStream(rets), size, monitor, false);
                b.setExecute(true);
                continue;
            }
            if (mem.getBlock(base) != null) continue; // the imported object
            byte[] data = Files.readAllBytes(Paths.get(dir, f[0]));
            MemoryBlock b = mem.createInitializedBlock(f[0], base,
                    new ByteArrayInputStream(data), data.length, monitor, false);
            b.setExecute(f[2].equals("code"));
            b.setWrite(!f[2].equals("code"));
            if (f[3].equals("16")) code16.add(f[0]);
        }
        SymbolTable st = currentProgram.getSymbolTable();
        for (String line : Files.readAllLines(Paths.get(dir, "symbols.txt"))) {
            String[] f = line.trim().split("\\s+", 2);
            if (f.length < 2) continue;
            Address a = toAddr(f[0]);
            MemoryBlock b = mem.getBlock(a);
            if (b == null) continue;
            String name = f[1].replaceAll("[^A-Za-z0-9_.@$?]", "_");
            Symbol s = st.createLabel(a, name, SourceType.IMPORTED);
            if (b.getName().equals("imports")) {
                disassemble(a);
                createFunction(a, name);
            } else if (b.isExecute() && !code16.contains(b.getName())) {
                st.addExternalEntryPoint(a);
                s.setPrimary();
            }
        }
    }
}
