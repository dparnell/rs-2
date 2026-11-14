* CHARACTER handling, COMMON and BLOCK DATA, subroutines and functions
* (adjustable arrays, EXTERNAL, ENTRY, alternate returns, SAVE,
* statement functions, EQUIVALENCE).  wfl386 -l=os2v2 fchars.for
      PROGRAM FCHARS
      INTEGER NFAIL
      COMMON /CHK/ NFAIL
      CHARACTER*20 S, T
      CHARACTER*5 SHORT
      CHARACTER*40 LONG
      CHARACTER*1 C
      CHARACTER*10 UPPER
      EXTERNAL UPPER
      INTEGER I, N, M(3, 4), ISUM, NEXT, IVAL
      INTEGER COUNTR, RESETC, LENOF
      REAL A(10), TABLE(4), SUMR, SQ, Q, Y
      EXTERNAL SQUARE
      REAL SQUARE, APPLY
      INTEGER*4 IW
      INTEGER*2 IH(2)
      EQUIVALENCE (IW, IH)
      INTEGER NBLK, IBLK(3)
      REAL RBLK
      CHARACTER*8 CBLK
      COMMON /BLK1/ NBLK, IBLK, RBLK
      COMMON /BLK2/ CBLK
*     A statement function
      SQ(Q) = Q * Q + 1.0
      NFAIL = 0
      PRINT *, 'Open Watcom FORTRAN 77 on rs2: characters, COMMON'

*     Substrings, concatenation, LEN, INDEX, ICHAR/CHAR
      S = 'Hello'
      T = S(1:4) // ', ' // 'world'
      CALL CHECKS('concatenation', T, 'Hell, world')
      CALL CHECKI('LEN', LEN(T), 20)
      CALL CHECKI('INDEX', INDEX(T, 'world'), 7)
      CALL CHECKI('INDEX missing', INDEX(T, 'xyz'), 0)
      CALL CHECKI('ICHAR', ICHAR('A'), 65)
      C = CHAR(ICHAR('a') + 25)
      CALL CHECKS('CHAR', C, 'z')
      SHORT = T
      CALL CHECKS('truncation', SHORT, 'Hell,')
      T(7:11) = 'WORLD'
      CALL CHECKS('substring assignment', T, 'Hell, WORLD')
      T(3:) = 'xyz'
      CALL CHECKS('open-ended substring', T, 'Hexyz')
      IF (.NOT. LGE('abc', 'ABC')) CALL FAIL('LGE')
      IF (.NOT. LLT('ABC', 'ABD')) CALL FAIL('LLT')
      IF ('ab' .NE. 'ab   ') CALL FAIL('blank padded comparison')
      CALL CHECKS('CHARACTER function', UPPER('mixed Case'),
     &            'MIXED CASE')
      CALL CHECKI('LEN of assumed length', LENOF('twelve chars'), 12)
      LONG = ' '
      DO 10 I = 1, 8
         LONG(I*5-4:I*5) = CHAR(48 + I) // '....'
   10 CONTINUE
      CALL CHECKS('built string', LONG,
     &            '1....2....3....4....5....6....7....8....')
      WRITE (S, '(I5.5,A)') 42, '-x'
      CALL CHECKS('internal WRITE', S, '00042-x')

*     COMMON initialised by BLOCK DATA
      CALL CHECKI('BLOCK DATA integer', NBLK, 3)
      CALL CHECKI('BLOCK DATA array', IBLK(2), 20)
      CALL CHECKR('BLOCK DATA real', RBLK, 2.5)
      CALL CHECKS('BLOCK DATA character', CBLK, 'blockdat')
      CALL BUMP
      CALL CHECKI('COMMON changed by a subroutine', IBLK(3), 31)

*     Adjustable arrays (2-D, column major), passing array sections
      DO 30 J = 1, 4
         DO 20 I = 1, 3
            M(I, J) = 10 * I + J
   20    CONTINUE
   30 CONTINUE
      CALL CHECKI('adjustable 2-D sum', ISUM(M, 3, 4), 270)
      CALL CHECKI('array element as array start', ISUM(M(1, 3), 3, 2),
     &            141)
      DO 40 I = 1, 10
         A(I) = I
   40 CONTINUE
      CALL CHECKR('assumed size', SUMR(A, 10), 55.0)

*     EXTERNAL: a function as an argument; statement function
      CALL CHECKR('EXTERNAL argument', APPLY(SQUARE, 3.0), 9.0)
      CALL CHECKR('statement function', SQ(2.0), 5.0)

*     SAVE and ENTRY
      N = COUNTR()
      N = COUNTR()
      N = COUNTR()
      CALL CHECKI('SAVEd counter', N, 3)
      N = RESETC(10)
      N = COUNTR()
      CALL CHECKI('ENTRY', N, 11)

*     Alternate returns
      CALL ALTRET(1, *50, *60)
      CALL FAIL('alternate return 1 not taken')
      GO TO 70
   50 CONTINUE
      CALL ALTRET(2, *80, *60)
   80 CALL FAIL('alternate return 2 not taken')
      GO TO 70
   60 CONTINUE
   70 CONTINUE

*     Computed and assigned GO TO, arithmetic IF
      IVAL = 0
      DO 100 I = 1, 3
         GO TO (91, 92, 93), I
   91    IVAL = IVAL + 1
         GO TO 100
   92    IVAL = IVAL + 10
         GO TO 100
   93    IVAL = IVAL + 100
  100 CONTINUE
      CALL CHECKI('computed GO TO', IVAL, 111)
      ASSIGN 110 TO NEXT
      GO TO NEXT, (110, 120)
  110 Y = -1.0
      IF (Y) 111, 120, 120
  111 CONTINUE
      GO TO 130
  120 CALL FAIL('arithmetic IF')
  130 CONTINUE

*     EQUIVALENCE: the low half comes first on the x86
      IW = 65536 * 3 + 7
      CALL CHECKI('EQUIVALENCE low', INT(IH(1)), 7)
      CALL CHECKI('EQUIVALENCE high', INT(IH(2)), 3)

*     DATA statement with a table
      CALL TABLES(TABLE)
      CALL CHECKR('DATA table', TABLE(3), 0.75)

      IF (NFAIL .EQ. 0) THEN
         PRINT *, 'FCHARS: all checks passed'
      ELSE
         PRINT *, 'FCHARS:', NFAIL, ' checks FAILED'
      END IF
      END

      BLOCK DATA INIT
      INTEGER NBLK, IBLK(3)
      REAL RBLK
      CHARACTER*8 CBLK
      COMMON /BLK1/ NBLK, IBLK, RBLK
      COMMON /BLK2/ CBLK
      DATA NBLK /3/, IBLK /10, 20, 30/, RBLK /2.5/
      DATA CBLK /'blockdat'/
      END

      SUBROUTINE BUMP
      INTEGER NBLK, IBLK(3)
      REAL RBLK
      COMMON /BLK1/ NBLK, IBLK, RBLK
      IBLK(NBLK) = IBLK(NBLK) + 1
      END

      CHARACTER*10 FUNCTION UPPER(S)
      CHARACTER*(*) S
      INTEGER I, K
      UPPER = S
      DO 10 I = 1, LEN(UPPER)
         K = ICHAR(UPPER(I:I))
         IF (K .GE. ICHAR('a') .AND. K .LE. ICHAR('z'))
     &      UPPER(I:I) = CHAR(K - 32)
   10 CONTINUE
      END

      INTEGER FUNCTION LENOF(S)
      CHARACTER*(*) S
      LENOF = LEN(S)
      END

      INTEGER FUNCTION ISUM(M, NI, NJ)
      INTEGER NI, NJ, M(NI, NJ), I, J
      ISUM = 0
      DO 20 J = 1, NJ
         DO 10 I = 1, NI
            ISUM = ISUM + M(I, J)
   10    CONTINUE
   20 CONTINUE
      END

      REAL FUNCTION SUMR(A, N)
      INTEGER N, I
      REAL A(*)
      SUMR = 0.0
      DO 10 I = 1, N
         SUMR = SUMR + A(I)
   10 CONTINUE
      END

      REAL FUNCTION SQUARE(X)
      REAL X
      SQUARE = X * X
      END

      REAL FUNCTION APPLY(F, X)
      REAL F, X
      EXTERNAL F
      APPLY = F(X)
      END

      INTEGER FUNCTION COUNTR()
      INTEGER RESETC, N, IC
      SAVE IC
      DATA IC /0/
      IC = IC + 1
      COUNTR = IC
      RETURN
      ENTRY RESETC(N)
      IC = N
      RESETC = IC
      END

      SUBROUTINE ALTRET(I, *, *)
      INTEGER I
      RETURN I
      END

      SUBROUTINE TABLES(T)
      REAL T(4), V(4)
      DATA V /0.25, 0.5, 0.75, 1.0/
      INTEGER I
      DO 10 I = 1, 4
         T(I) = V(I)
   10 CONTINUE
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
