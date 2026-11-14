* Mixed Fortran and C: Fortran calls C functions (arguments by
* reference, in registers as Watcom's default), C calls back a Fortran
* subroutine.  wcc386 cmix.c, then wfl386 -l=os2v2 fmix.for cmix.obj
      PROGRAM FMIX
      INTEGER NFAIL
      COMMON /CHK/ NFAIL
      INTEGER CADD, CSUMV, N, IV(5), I
      DOUBLE PRECISION D, DV(4)
      EXTERNAL FSCALE
      NFAIL = 0
      PRINT *, 'Open Watcom FORTRAN 77 on rs2: Fortran + C'
      N = CADD(40, 2)
      IF (N .NE. 42) CALL FAIL('C function result')
      DO 10 I = 1, 5
         IV(I) = I * I
   10 CONTINUE
      IF (CSUMV(IV, 5) .NE. 55) CALL FAIL('array passed to C')
      D = 2.0D0
      CALL CHALF(D)
      IF (D .NE. 1.0D0) CALL FAIL('DOUBLE PRECISION changed by C')
      DO 20 I = 1, 4
         DV(I) = I
   20 CONTINUE
*     C calls FSCALE for each element, which multiplies by 2.5
      CALL CEACH(DV, 4)
      IF (ABS(DV(4) - 10.0D0) .GT. 1.0D-12) CALL FAIL('C called back')
      CALL CPRINT(DV(3))
      IF (NFAIL .EQ. 0) THEN
         PRINT *, 'FMIX: all checks passed'
      ELSE
         PRINT *, 'FMIX:', NFAIL, ' checks FAILED'
      END IF
      END

      SUBROUTINE FSCALE(X)
      DOUBLE PRECISION X
      X = X * 2.5D0
      END

      SUBROUTINE FAIL(WHAT)
      CHARACTER*(*) WHAT
      INTEGER NFAIL
      COMMON /CHK/ NFAIL
      NFAIL = NFAIL + 1
      PRINT *, 'FAIL: ', WHAT
      END
