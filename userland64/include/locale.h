#ifndef _LOCALE_H
#define _LOCALE_H
#include "_rs2libc.h"
_RS2_BEGIN
/* Only the "C" locale. */
struct lconv {
    char *decimal_point, *thousands_sep, *grouping, *int_curr_symbol, *currency_symbol;
    char *mon_decimal_point, *mon_thousands_sep, *mon_grouping, *positive_sign, *negative_sign;
    char int_frac_digits, frac_digits, p_cs_precedes, p_sep_by_space, n_cs_precedes, n_sep_by_space;
    char p_sign_posn, n_sign_posn, int_p_cs_precedes, int_p_sep_by_space, int_n_cs_precedes;
    char int_n_sep_by_space, int_p_sign_posn, int_n_sign_posn;
};
#define LC_ALL 0
#define LC_COLLATE 1
#define LC_CTYPE 2
#define LC_MONETARY 3
#define LC_NUMERIC 4
#define LC_TIME 5
#define LC_MESSAGES 6
#define LC_COLLATE_MASK 2
#define LC_CTYPE_MASK 4
#define LC_MONETARY_MASK 8
#define LC_NUMERIC_MASK 16
#define LC_TIME_MASK 32
#define LC_MESSAGES_MASK 64
#define LC_ALL_MASK 126
typedef struct _rs2_locale *locale_t;
#define LC_GLOBAL_LOCALE ((locale_t)-1)
locale_t newlocale(int, const char *, locale_t);
locale_t duplocale(locale_t);
void freelocale(locale_t);
locale_t uselocale(locale_t);
char *setlocale(int, const char *);
struct lconv *localeconv(void);
_RS2_END
#endif
