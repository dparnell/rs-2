* DOUBLE PRECISION and COMPLEX arithmetic and the intrinsic functions,
* checked against known values.  wfl386 -l=os2v2 fmath.for
      PROGRAM FMATH
      INTEGER NFAIL
      COMMON /CHK/ NFAIL
      DOUBLE PRECISION PI, D, S, T, X
      REAL R
      COMPLEX C, Z
      DOUBLE COMPLEX DZ
      INTEGER I, K
      INTEGER*2 I2
      NFAIL = 0
      PRINT *, 'Open Watcom FORTRAN 77 on rs2: math'

      PI = 4.0D0 * ATAN(1.0D0)
      CALL CHECKD('pi', PI, 3.14159265358979323846D0)
      CALL CHECKD('DSQRT(2)', DSQRT(2.0D0), 1.41421356237309505D0)
      CALL CHECKD('EXP(1)', EXP(1.0D0), 2.71828182845904524D0)
      CALL CHECKD('LOG(10)', LOG(10.0D0), 2.30258509299404568D0)
      CALL CHECKD('LOG10(2)', LOG10(2.0D0), 0.301029995663981195D0)
      CALL CHECKD('SIN(pi/6)', SIN(PI / 6), 0.5D0)
      CALL CHECKD('COS(pi/3)', COS(PI / 3), 0.5D0)
      CALL CHECKD('TAN(pi/4)', TAN(PI / 4), 1.0D0)
      CALL CHECKD('ASIN(1)', ASIN(1.0D0), PI / 2)
      CALL CHECKD('ACOS(-1)', ACOS(-1.0D0), PI)
      CALL CHECKD('ATAN2(1,-1)', ATAN2(1.0D0, -1.0D0), 0.75D0 * PI)
      CALL CHECKD('SINH(1)', SINH(1.0D0), 1.17520119364380146D0)
      CALL CHECKD('COSH(1)', COSH(1.0D0), 1.54308063481524378D0)
      CALL CHECKD('TANH(0.5)', TANH(0.5D0), 0.462117157260009758D0)
      CALL CHECKD('2**0.5', 2.0D0 ** 0.5D0, DSQRT(2.0D0))
      CALL CHECKD('3**-2', 3.0D0 ** (-2), 1.0D0 / 9)
      CALL CHECKD('DMOD', DMOD(10.5D0, 3.0D0), 1.5D0)
      CALL CHECKD('DINT', DINT(-2.7D0), -2.0D0)
      CALL CHECKD('DNINT', DNINT(-2.5D0), -3.0D0)
      CALL CHECKI('NINT', NINT(2.5), 3)
      CALL CHECKI('IDNINT', IDNINT(-3.5D0), -4)
      CALL CHECKI('INT', INT(-3.9), -3)
      CALL CHECKI('MOD', MOD(-7, 3), -1)
      CALL CHECKI('ISIGN', ISIGN(5, -2), -5)
      CALL CHECKI('IDIM', IDIM(3, 8), 0)
      CALL CHECKI('MAX0', MAX0(3, 9, -1), 9)
      CALL CHECKI('MIN0', MIN0(3, 9, -1), -1)
      CALL CHECKD('DSIGN', DSIGN(2.0D0, -0.0D0 - 1), -2.0D0)
      CALL CHECKD('DPROD', DPROD(1.5, 2.5), 3.75D0)
      CALL CHECKR('AMAX1', AMAX1(1.5, -2.0, 7.25), 7.25)
      CALL CHECKR('REAL SQRT', SQRT(16.0), 4.0)
      CALL CHECKR('REAL ALOG', ALOG(1.0), 0.0)

*     A series: sum of 1/k**2 to 100000 terms (pi**2/6 - 1/100000...)
      S = 0.0D0
      DO 10 K = 100000, 1, -1
         S = S + 1.0D0 / (DBLE(K) * K)
   10 CONTINUE
      CALL CHECKN('sum 1/k**2', S, PI * PI / 6 - 1.0D0 / 100000,
     &            1.0D-9)

*     Newton's method for the cube root of 2
      X = 1.0D0
      DO 20 I = 1, 50
         T = X - (X ** 3 - 2) / (3 * X ** 2)
         IF (ABS(T - X) .LT. 1.0D-15) GO TO 25
         X = T
   20 CONTINUE
   25 CALL CHECKD('cube root of 2', X, 1.25992104989487316D0)

*     COMPLEX arithmetic and intrinsics
      C = (3.0, 4.0)
      CALL CHECKR('CABS', CABS(C), 5.0)
      Z = C * (1.0, -2.0)
      CALL CHECKC('complex multiply', Z, (11.0, -2.0))
      Z = C / (0.0, 1.0)
      CALL CHECKC('complex divide', Z, (4.0, -3.0))
      CALL CHECKC('CONJG', CONJG(C), (3.0, -4.0))
      CALL CHECKC('CSQRT', CSQRT((-4.0, 0.0)), (0.0, 2.0))
      CALL CHECKC('CSQRT(3+4i)', CSQRT(C), (2.0, 1.0))
      Z = CEXP(CMPLX(0.0, REAL(PI)))
      CALL CHECKC('CEXP(i pi)', Z, (-1.0, 0.0))
      Z = CLOG((0.0, 1.0))
      CALL CHECKC('CLOG(i)', Z, CMPLX(0.0, REAL(PI / 2)))
      CALL CHECKR('AIMAG', AIMAG(C), 4.0)
      CALL CHECKR('REAL', REAL(C), 3.0)
      Z = C ** 2
      CALL CHECKC('complex power', Z, (-7.0, 24.0))
      Z = CSIN((1.0, 1.0))
      CALL CHECKC('CSIN', Z, (1.29845758, 0.63496391))
      Z = CCOS((1.0, 1.0))
      CALL CHECKC('CCOS', Z, (0.83373003, -0.98889771))

*     DOUBLE COMPLEX (a Watcom extension)
      DZ = (1.0D0, 2.0D0) * (3.0D0, -1.0D0)
      CALL CHECKD('double complex re', DBLE(DZ), 5.0D0)
      CALL CHECKD('double complex im', DIMAG(DZ), 5.0D0)
      DZ = CDSQRT((0.0D0, 2.0D0))
      CALL CHECKD('CDSQRT re', DBLE(DZ), 1.0D0)
      CALL CHECKD('CDSQRT im', DIMAG(DZ), 1.0D0)

*     Mixed-mode and integer arithmetic
      D = 7 / 2 + 7.0 / 2 + 7.0D0 / 4
      CALL CHECKD('mixed mode', D, 8.25D0)
      I2 = 32767
      I = I2 + 1
      CALL CHECKI('INTEGER*2 widened', I, 32768)
      K = 2 ** 30
      CALL CHECKI('2**30', K, 1073741824)
      R = 1.0E-38
      R = R * 1.0E-5
      IF (R .GT. 1.0E-42 .OR. R .LT. 0.0) CALL FAIL('underflow')

*     Printed with the list-directed and E formats
      WRITE (*, '(1X, A, F20.16, E25.16)') 'pi =', PI, PI
      WRITE (*, '(1X, A, 2F10.5)') 'CSQRT(3+4i) =', CSQRT(C)
      PRINT *, 'exp(1) =', EXP(1.0D0)

      IF (NFAIL .EQ. 0) THEN
         PRINT *, 'FMATH: all checks passed'
      ELSE
         PRINT *, 'FMATH:', NFAIL, ' checks FAILED'
      END IF
      END

      SUBROUTINE FAIL(WHAT)
      CHARACTER*(*) WHAT
      INTEGER NFAIL
      COMMON /CHK/ NFAIL
      NFAIL = NFAIL + 1
      PRINT *, 'FAIL: ', WHAT
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

      SUBROUTINE CHECKC(WHAT, GOT, WANT)
      CHARACTER*(*) WHAT
      COMPLEX GOT, WANT
      IF (CABS(GOT - WANT) .GT. 1.0E-5 * MAX(1.0, CABS(WANT))) THEN
         CALL FAIL(WHAT)
         PRINT *, '  got', GOT, ' want', WANT
      END IF
      END

      SUBROUTINE CHECKD(WHAT, GOT, WANT)
      CHARACTER*(*) WHAT
      DOUBLE PRECISION GOT, WANT
      CALL CHECKN(WHAT, GOT, WANT, 1.0D-14)
      END

      SUBROUTINE CHECKN(WHAT, GOT, WANT, TOL)
      CHARACTER*(*) WHAT
      DOUBLE PRECISION GOT, WANT, TOL
      IF (ABS(GOT - WANT) .GT. TOL * MAX(1.0D0, ABS(WANT))) THEN
         CALL FAIL(WHAT)
         PRINT *, '  got', GOT, ' want', WANT
      END IF
      END
