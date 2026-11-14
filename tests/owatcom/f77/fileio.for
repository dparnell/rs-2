* File I/O: OPEN/CLOSE, sequential (formatted and unformatted), direct
* access, INQUIRE, REWIND, BACKSPACE, ENDFILE, END=/ERR=/IOSTAT=.
* wfl386 -l=os2v2 fileio.for
      PROGRAM FILEIO
      INTEGER NFAIL
      COMMON /CHK/ NFAIL
      INTEGER I, J, N, IOS, IREC, NREC
      REAL R
      DOUBLE PRECISION D, DA(100)
      CHARACTER*40 NAME, LINE
      CHARACTER*12 ACC, FORM, SEQ, DIR
      LOGICAL EX, OP
      NFAIL = 0
      PRINT *, 'Open Watcom FORTRAN 77 on rs2: file I/O'

*     A file that does not exist
      INQUIRE (FILE='NOSUCH.DAT', EXIST=EX)
      IF (EX) CALL FAIL('INQUIRE EXIST of a missing file')
      OPEN (UNIT=9, FILE='NOSUCH.DAT', STATUS='OLD', IOSTAT=IOS)
      IF (IOS .EQ. 0) CALL FAIL('OPEN OLD of a missing file')

*     Sequential formatted: write 100 lines, read them back
      OPEN (UNIT=10, FILE='SEQ.DAT', STATUS='UNKNOWN',
     &      FORM='FORMATTED', ACCESS='SEQUENTIAL')
      DO 10 I = 1, 100
         WRITE (10, 100) I, SQRT(REAL(I)), 'line'
   10 CONTINUE
  100 FORMAT (I4, F10.5, 1X, A)
      ENDFILE (10)
      REWIND (10)
      N = 0
   20 READ (10, 100, END=30, ERR=90) J, R, LINE
         N = N + 1
         IF (J .NE. N) CALL FAIL('sequential read order')
         IF (ABS(R - SQRT(REAL(N))) .GT. 1.0E-5)
     &      CALL FAIL('sequential read value')
         IF (LINE .NE. 'line') CALL FAIL('sequential read text')
         GO TO 20
   30 CALL CHECKI('lines read before END=', N, 100)

*     BACKSPACE over the last record, then rewrite it (truncates)
      REWIND (10)
      DO 40 I = 1, 50
         READ (10, 100) J
   40 CONTINUE
      BACKSPACE (10)
      READ (10, 100) J
      CALL CHECKI('BACKSPACE then READ', J, 50)
      WRITE (10, 100) 999, 0.0, 'last'
      REWIND (10)
      N = 0
   50 READ (10, 100, END=60) J
         N = N + 1
         GO TO 50
   60 CALL CHECKI('lines after a write in the middle', N, 51)
      INQUIRE (UNIT=10, OPENED=OP, NAME=NAME, ACCESS=ACC, FORM=FORM,
     &         SEQUENTIAL=SEQ, DIRECT=DIR)
      IF (.NOT. OP) CALL FAIL('INQUIRE OPENED')
      IF (INDEX(NAME, 'SEQ.DAT') .EQ. 0) CALL FAIL('INQUIRE NAME')
      CALL CHECKS('INQUIRE ACCESS', ACC, 'SEQUENTIAL')
      CALL CHECKS('INQUIRE FORM', FORM, 'FORMATTED')
      CLOSE (10)
      INQUIRE (FILE='SEQ.DAT', EXIST=EX, OPENED=OP)
      IF (.NOT. EX) CALL FAIL('INQUIRE EXIST after CLOSE')
      IF (OP) CALL FAIL('INQUIRE OPENED after CLOSE')

*     Sequential unformatted records of different lengths
      OPEN (11, FILE='UNF.DAT', FORM='UNFORMATTED', STATUS='NEW',
     &      IOSTAT=IOS)
      IF (IOS .NE. 0) THEN
         OPEN (11, FILE='UNF.DAT', FORM='UNFORMATTED')
         CLOSE (11, STATUS='DELETE')
         OPEN (11, FILE='UNF.DAT', FORM='UNFORMATTED', STATUS='NEW')
      END IF
      DO 70 I = 1, 10
         DA(I) = 1.0D0 / I
   70 CONTINUE
      WRITE (11) 3, (DA(I), I = 1, 3)
      WRITE (11) 10, (DA(I), I = 1, 10)
      WRITE (11) 'text record'
      REWIND (11)
      DO 75 I = 1, 10
         DA(I) = 0.0D0
   75 CONTINUE
      READ (11) N
      CALL CHECKI('unformatted record 1', N, 3)
      READ (11) N, (DA(I), I = 1, N)
      CALL CHECKI('unformatted record 2', N, 10)
      CALL CHECKD('unformatted DA(7)', DA(7), 1.0D0 / 7)
      READ (11) LINE(1:11)
      CALL CHECKS('unformatted text', LINE(1:11), 'text record')
      READ (11, IOSTAT=IOS) N
      IF (IOS .GE. 0) CALL FAIL('IOSTAT at end of file')
      CLOSE (11, STATUS='DELETE')
      INQUIRE (FILE='UNF.DAT', EXIST=EX)
      IF (EX) CALL FAIL('CLOSE STATUS=DELETE')

*     Direct access: write records out of order, read them back
      OPEN (12, FILE='DIR.DAT', ACCESS='DIRECT', RECL=16,
     &      FORM='UNFORMATTED', STATUS='UNKNOWN')
      DO 80 IREC = 20, 1, -1
         WRITE (12, REC=IREC) IREC, DBLE(IREC) ** 2
   80 CONTINUE
      READ (12, REC=7) J, D
      CALL CHECKI('direct record 7', J, 7)
      CALL CHECKD('direct value 7', D, 49.0D0)
      WRITE (12, REC=7) -7, -1.0D0
      READ (12, REC=8) J
      CALL CHECKI('direct record 8', J, 8)
      READ (12, REC=7) J, D
      CALL CHECKI('direct rewritten 7', J, -7)
      INQUIRE (12, ACCESS=ACC, RECL=N, NEXTREC=NREC)
      CALL CHECKS('INQUIRE direct ACCESS', ACC, 'DIRECT')
      CALL CHECKI('INQUIRE RECL', N, 16)
      CALL CHECKI('INQUIRE NEXTREC', NREC, 8)
      CLOSE (12)

*     Formatted direct access, reopened
      OPEN (13, FILE='FDIR.DAT', ACCESS='DIRECT', RECL=20,
     &      FORM='FORMATTED')
      DO 85 IREC = 1, 5
         WRITE (13, '(A6,I6,F8.2)', REC=IREC) 'rec', IREC, IREC*1.5
   85 CONTINUE
      CLOSE (13)
      OPEN (13, FILE='FDIR.DAT', ACCESS='DIRECT', RECL=20,
     &      FORM='FORMATTED', STATUS='OLD')
      READ (13, '(A6,I6,F8.2)', REC=4) LINE(1:6), J, R
      CALL CHECKS('formatted direct text', LINE(1:6), '   rec')
      CALL CHECKI('formatted direct int', J, 4)
      CALL CHECKR('formatted direct real', R, 6.0)
      READ (13, '(A6)', REC=9, IOSTAT=IOS) LINE(1:6)
      IF (IOS .EQ. 0) CALL FAIL('reading a record past the end')
      CLOSE (13, STATUS='DELETE')

*     Appending to a file opened again (ACCESS='APPEND', a Watcom
*     extension)
      OPEN (14, FILE='SEQ.DAT', STATUS='OLD', ACCESS='APPEND')
      WRITE (14, '(A)') 'appended'
      CLOSE (14)
      OPEN (14, FILE='SEQ.DAT', STATUS='OLD')
      N = 0
   89 READ (14, '(A)', END=91) LINE
         N = N + 1
         GO TO 89
   91 CALL CHECKI('lines after appending', N, 52)
      CALL CHECKS('appended line', LINE, 'appended')
      CLOSE (14, STATUS='DELETE')

      IF (NFAIL .EQ. 0) THEN
         PRINT *, 'FILEIO: all checks passed'
      ELSE
         PRINT *, 'FILEIO:', NFAIL, ' checks FAILED'
      END IF
      STOP
   90 CALL FAIL('ERR= taken while reading')
      END

      SUBROUTINE FAIL(WHAT)
      CHARACTER*(*) WHAT
      INTEGER NFAIL
      COMMON /CHK/ NFAIL
      NFAIL = NFAIL + 1
      PRINT *, 'FAIL: ', WHAT
      END

      SUBROUTINE CHECKS(WHAT, GOT, WANT)
      CHARACTER*(*) WHAT, GOT, WANT
      IF (GOT .NE. WANT) THEN
         CALL FAIL(WHAT)
         PRINT *, '  got  [', GOT, ']'
         PRINT *, '  want [', WANT, ']'
      END IF
      END

      SUBROUTINE CHECKI(WHAT, GOT, WANT)
      CHARACTER*(*) WHAT
      INTEGER GOT, WANT
      IF (GOT .NE. WANT) THEN
         CALL FAIL(WHAT)
         PRINT *, '  got', GOT, ' want', WANT
      END IF
      END

      SUBROUTINE CHECKR(WHAT, GOT, WANT)
      CHARACTER*(*) WHAT
      REAL GOT, WANT
      IF (ABS(GOT - WANT) .GT. 1.0E-5 * MAX(1.0, ABS(WANT))) THEN
         CALL FAIL(WHAT)
         PRINT *, '  got', GOT, ' want', WANT
      END IF
      END

      SUBROUTINE CHECKD(WHAT, GOT, WANT)
      CHARACTER*(*) WHAT
      DOUBLE PRECISION GOT, WANT
      IF (ABS(GOT - WANT) .GT. 1.0D-13 * MAX(1.0D0, ABS(WANT))) THEN
         CALL FAIL(WHAT)
         PRINT *, '  got', GOT, ' want', WANT
      END IF
      END
