# wmake: implicit rules from an !include'd file, macros, .BEFORE, !ifdef
!include mkrules.mif

OBJS = asmmain.obj asmfn.obj libfn.obj

.BEFORE
	@echo wmake: starting

mk2.exe: $(OBJS)
	wlink op q system os2v2 name $@ file { $(OBJS) }
	@echo wmake: linked $@

.asm.obj:
	wasm -zq $[@

!ifdef SHOWDEFS
!error SHOWDEFS should not be defined
!endif

clean: .SYMBOLIC
	-del $(OBJS) mk2.exe
