"""Print synthetic P11 reference vectors; standard library only, no files/network.

Independent Decimal(precision=65) evaluation, not Rust/libm or an IV round trip.
Phi uses an integral power series with exp factor rather than erfc. The printed
binary64 constants qualify a finite envelope, not arbitrary market inputs.
"""
from decimal import Decimal as D, localcontext


def reference(spot, strike, rate, yield_rate, seconds, volatility):
    with localcontext() as context:
        context.prec = 65
        s, k, r, q, seconds, sigma = map(D, (spot, strike, rate, yield_rate, seconds, volatility))
        t = seconds / D(31536000)
        pi = D("3.141592653589793238462643383279502884197169399375105820974944592307816")
        normalizer = (2 * pi).sqrt()

        def phi(x):
            return (-x * x / 2).exp() / normalizer

        def cdf(x):
            term = total = x
            for n in range(1, 4096):
                term *= x * x / (2 * n + 1)
                total += term
                if abs(term) < D("1e-62"):
                    return D("0.5") + phi(x) * total
            raise ArithmeticError("reference series did not converge")

        root = t.sqrt()
        d1 = ((s / k).ln() + (r - q + sigma * sigma / 2) * t) / (sigma * root)
        d2 = d1 - sigma * root
        a, b = s * (-q * t).exp(), k * (-r * t).exp()
        gamma = (-q * t).exp() * phi(d1) / (s * sigma * root)
        vega = a * phi(d1) * root
        common = -a * phi(d1) * sigma / (2 * root)
        call = (a * cdf(d1) - b * cdf(d2), (-q * t).exp() * cdf(d1), gamma,
                vega, common - r * b * cdf(d2) + q * a * cdf(d1), t * b * cdf(d2))
        put = (b * cdf(-d2) - a * cdf(-d1), -(-q * t).exp() * cdf(-d1), gamma,
               vega, common + r * b * cdf(-d2) - q * a * cdf(-d1), -t * b * cdf(-d2))
        return call, put


if __name__ == "__main__":
    for parameters in [
        ("100", "100", ".05", "0", "31536000", ".2"),
        ("910", "980", ".02", ".025", "7884000", ".25"),
        ("5000", "5000", ".03", ".01", "60", ".35"),
        ("80", "100", "-.01", "-.02", "15768000", ".4"),
    ]:
        call, put = reference(*parameters)
        print("(", ", ".join(str(float(p)) for p in parameters), ",")
        for values in (call, put):
            print("    [" + ", ".join(format(float(v), ".17g") for v in values) + "],")
        print("),")
