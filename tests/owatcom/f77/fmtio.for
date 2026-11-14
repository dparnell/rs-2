* Formatted I/O: WRITE/READ with FORMAT, list-directed and internal
* files.  wfl386 -l=os2v2 fmtio.for
      PROGRAM FMTIO
      INTEGER NFAIL
      COMMON /CHK/ NFAIL
      CHARACTER*80 LINE
      CHARACTER*12 WORD
      INTEGER I, J, K, IA(6)
      REAL X, Y
      DOUBLE PRECISION D, E
      LOGICAL L
      NFAIL = 0
      PRINT *, 'Open Watcom FORTRAN 77 on rs2: formatted I/O'

*     Integer, fixed, exponent and logical edit descriptors
      WRITE (LINE, 100) 42, -7, 3.25, 1.5E10, .TRUE.
  100 FORMAT (I5, I4.3, F8.3, E12.4, L3)
      CALL CHECKS('I/F/E/L', LINE(1:32),
     &            '   42-007   3.250  0.1500E+11  T')

*     D and G editing, scale factor, X and T positioning
      D = 1234.5678D0
      WRITE (LINE, 110) D, D, 0.0625
  110 FORMAT (D15.7, 1X, G12.5, T32, 1PE10.3)
      CALL CHECKS('D/G/T/P', LINE(1:41),
     &            '  0.1234568D+04   1234.6        6.250E-02')

*     Character editing and apostrophes, Hollerith
      WRITE (LINE, 120) 'abc', 'xyz'
  120 FORMAT ('[', A, ']', A5, '|', 3HHOL, '''')
      CALL CHECKS('A/H', LINE(1:16), '[abc]  xyz|HOL''')

*     Reading formatted fields, with blanks as zeros (BZ) and implied
*     decimal point
      LINE = '  123 4.5  12345 1 2'
      READ (LINE, 130) I, X, Y, J
  130 FORMAT (I5, F4.1, F7.2, BZ, I4)
      CALL CHECKI('read I5', I, 123)
      CALL CHECKR('read F4.1', X, 4.5)
      CALL CHECKR('read F7.2 implied', Y, 123.45)
      CALL CHECKI('read BZ I4', J, 102)

*     List-directed READ: repeat counts, null values, slash
      DO 10 K = 1, 6
         IA(K) = -1
   10 CONTINUE
      LINE = '3*7, ,9 /'
      READ (LINE, *) IA
      CALL CHECKI('list repeat 1', IA(1), 7)
      CALL CHECKI('list repeat 3', IA(3), 7)
      CALL CHECKI('list null', IA(4), -1)
      CALL CHECKI('list 5th', IA(5), 9)
      CALL CHECKI('list slash', IA(6), -1)
      LINE = ' 2.5D0, ''it''''s'', T'
      READ (LINE, *) E, WORD, L
      CALL CHECKD('list double', E, 2.5D0)
      CALL CHECKS('list char', WORD, 'it''s')
      IF (.NOT. L) CALL FAIL('list logical')

*     Implied DO in I/O lists, repeated groups, colon editing
      WRITE (LINE, 140) (K, K*K, K = 1, 3)
  140 FORMAT (3(I2, ':', I3, :, ','))
      CALL CHECKS('implied DO', LINE(1:20), ' 1:  1, 2:  4, 3:  9')

*     Rounding of F and E output
      WRITE (LINE, 150) 2.6, 0.126, -0.0005, 99999.7
  150 FORMAT (F4.0, F6.2, F8.3, F9.0)
      CALL CHECKS('rounding', LINE(1:27),
     &            '  3.  0.13  -0.001  100000.')

*     Output that does not fit gives asterisks
      WRITE (LINE, 160) 12345, 1.0E5
  160 FORMAT (I3, F5.1)
      CALL CHECKS('overflow', LINE(1:8), '********')

*     Sign control and the list-directed form of numbers
      WRITE (LINE, 170) 5, 5, 5
  170 FORMAT (SP, I3, SS, I3, S, I3)
      CALL CHECKS('SP/SS', LINE(1:9), ' +5  5  5')

*     Standard output with a FORMAT
      WRITE (*, 180) 'pi', 4.0D0*ATAN(1.0D0)
  180 FORMAT (1X, A, ' = ', F18.15)

      IF (NFAIL .EQ. 0) THEN
         PRINT *, 'FMTIO: all checks passed'
      ELSE
         PRINT *, 'FMTIO:', NFAIL, ' checks FAILED'
      END IF
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
