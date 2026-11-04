#include <stdint.h>
#include <stddef.h>

int64_t fastcdc_cortar(const uint8_t *buf, int64_t n, int64_t minimo, int64_t normal, int64_t maximo,
                       uint64_t mascara_s, uint64_t mascara_l, const uint64_t *gear,
                       int64_t *offs, int64_t *tams, int64_t cap)
{
    int64_t pos = 0;
    int64_t k = 0;
    while (pos < n) {
        int64_t resto = n - pos;
        int64_t len;
        if (resto <= minimo) {
            len = resto;
        } else {
            int64_t lim = resto < maximo ? resto : maximo;
            int64_t nor = normal < lim ? normal : lim;
            const uint8_t *p = buf + pos;
            uint64_t fp = 0;
            int64_t i = minimo;
            len = lim;
            int achou = 0;
            for (; i < nor; i++) {
                fp = (fp << 1) + gear[p[i]];
                if (!(fp & mascara_s)) {
                    len = i + 1;
                    achou = 1;
                    break;
                }
            }
            if (!achou) {
                for (; i < lim; i++) {
                    fp = (fp << 1) + gear[p[i]];
                    if (!(fp & mascara_l)) {
                        len = i + 1;
                        break;
                    }
                }
            }
        }
        if (k >= cap) {
            return -1;
        }
        offs[k] = pos;
        tams[k] = len;
        k++;
        pos += len;
    }
    return k;
}
