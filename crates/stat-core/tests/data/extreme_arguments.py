"""Reference values for crates/stat-core/tests/extreme_arguments.rs, from the
defining integrals and closed forms with mpmath at 60 significant digits.
Run: python3 extreme_arguments.py (needs mpmath).
"""
import mpmath as mp
mp.mp.dps=60
# t sf with df=1e19 at 10: integrate density? use betainc: sf = 0.5*I_{v/(v+t^2)}(v/2, 1/2)
v=mp.mpf(10)**19; t=mp.mpf(10)
print("t_sf_1e19", mp.nstr(0.5*mp.betainc(v/2,0.5,0,v/(v+t*t),regularized=True),17))
print("cauchy_q_1e-20", mp.nstr(mp.tan(mp.pi*(mp.mpf('1e-20')-0.5)),17))
print("cauchy_q_1e-10", mp.nstr(mp.tan(mp.pi*(mp.mpf('1e-10')-0.5)),17))
# noncentral t sf: P(T>t) = ∫ P(Z > ... ) : T=(Z+δ)/sqrt(V/ν); P(T>t)=E_V[Φ̄(t sqrt(V/ν) - δ)]
def nct_sf(t,nu,d):
    nu=mp.mpf(nu); t=mp.mpf(t); d=mp.mpf(d)
    # V ~ chi2(nu): density
    f=lambda x: mp.exp((nu/2-1)*mp.log(x)-x/2-(nu/2)*mp.log(2)-mp.loggamma(nu/2))*mp.ncdf(-(t*mp.sqrt(x/nu)-d))
    return mp.quad(f,[0,mp.mpf('1e-30'),mp.mpf('1e-20'),mp.mpf('1e-10'),1e-4,0.01,1,10,100,mp.inf])
for (t,nu,d) in [(1e9,2,1),(1e5,5,2),(50,10,1.5)]:
    print("nct_sf",t,nu,d, mp.nstr(nct_sf(t,nu,d),17))
# t with df=1e200 not; StudentsT(5).cdf(1e200)->1; sf(1e200) = ? ~ C * t^-5
nu=5; t=mp.mpf('1e200')
print("t5_sf_1e200", mp.nstr(0.5*mp.betainc(mp.mpf(nu)/2,0.5,0,nu/(nu+t*t),regularized=True),17))
nu=1; t=mp.mpf('1e200')
print("t1_sf_1e200", mp.nstr(0.5*mp.betainc(mp.mpf(nu)/2,0.5,0,nu/(nu+t*t),regularized=True),17))
# F(3,7) sf at 1e310? f finite max 1.7e308; d1*f overflow with d1=3, f=1e308
d1,d2=mp.mpf(10),mp.mpf('0.5'); f=mp.mpf('1e308')
print("f_sf_1e308", mp.nstr(mp.betainc(d2/2,d1/2,0,d2/(d2+d1*f),regularized=True),17))
# chi2(0.002) quantile 0.6
k=mp.mpf('0.002')
q=mp.findroot(lambda lx: mp.log(mp.gammainc(k/2,0,mp.exp(lx)/2,regularized=True))-mp.log(mp.mpf('0.6')), mp.log(mp.mpf('1.6e-222')))
print("chi2_q", mp.nstr(mp.exp(q),17))
# ncx2(0.02,1) quantile 0.05
k=mp.mpf('0.02'); lam=mp.mpf(1)
def ncx2cdf(x):
    return mp.nsum(lambda j: mp.exp(-lam/2)*(lam/2)**j/mp.factorial(j)*mp.gammainc(k/2+j,0,x/2,regularized=True),[0,mp.inf])
q=mp.findroot(lambda lx: mp.log(ncx2cdf(mp.exp(lx)))-mp.log(mp.mpf('0.05')), mp.log(mp.mpf('4.6e-109')))
print("ncx2_q", mp.nstr(mp.exp(q),17))
# t df=0.01 quantiles at p=0.9 and 0.6
for p in ['0.6','0.9']:
    nu=mp.mpf('0.01')
    sf=lambda lt: 0.5*mp.betainc(nu/2,0.5,0,1/(1+mp.exp(2*lt)/nu),regularized=True)
    lt=mp.findroot(lambda lt: mp.log(sf(lt))-mp.log(1-mp.mpf(p)), 10)
    print("t001_q",p, mp.nstr(mp.exp(lt),17))
# Weibull shape 0.01 scale 1e-100 variance
k=mp.mpf('0.01'); lam=mp.mpf('1e-100')
print("weib_var", mp.nstr(lam**2*(mp.gamma(1+2/k)-mp.gamma(1+1/k)**2),17))
# loglogistic mu=1 sigma=1e-9
mu=1; s=mp.mpf('1e-9'); a=mp.pi*s
print("ll_var", mp.nstr(mp.exp(2*mu)*(2*a/mp.sin(2*a)-(a/mp.sin(a))**2),17))
s=mp.mpf('0.02'); a=mp.pi*s
print("ll_var_0.02", mp.nstr(mp.exp(2*mu)*(2*a/mp.sin(2*a)-(a/mp.sin(a))**2),17))
s=mp.mpf('0.1'); a=mp.pi*s
print("ll_var_0.1", mp.nstr(mp.exp(2*mu)*(2*a/mp.sin(2*a)-(a/mp.sin(a))**2),17))
# Gamma(0.005) P(X < 4.94e-324)
print("gamma_underflow_p", mp.nstr(mp.gammainc(mp.mpf('0.005'),0,mp.mpf(2)**-1075,regularized=True),10))
print("normal_sf_10", mp.nstr(mp.ncdf(-10),17))
