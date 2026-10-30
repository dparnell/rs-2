# wmake: builds the three test programs
all: hello.exe hello16.exe pmhello.exe .SYMBOLIC

hello.exe: hello.c
	wcl386 -zq -l=os2v2 hello.c

hello16.exe: hello16.c
	wcl -zq -l=os2 -i=$(%WATCOM)\h\os21x hello16.c

pmhello.exe: pmhello.c
	wcl386 -zq -l=os2v2_pm pmhello.c

clean: .SYMBOLIC
	del *.obj *.exe
