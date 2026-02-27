* Scc instruction tests (all 16 conditions)
	.text

* ─── Scc to data register ────────────────────────────────────────────────────
	st	d0
	sf	d0
	seq	d0
	sne	d0
	slt	d0
	sgt	d0
	sle	d0
	sge	d0
	scc	d0
	scs	d0
	smi	d0
	spl	d0
	svs	d0
	svc	d0
	shi	d0
	sls	d0

* ─── Scc to memory ───────────────────────────────────────────────────────────
	st	(a0)
	sf	(a0)
	seq	(a0)
	sne	(a0)
	slt	(a0)
	sgt	(a0)
	sle	(a0)
	sge	(a0)
	scc	(a0)
	scs	(a0)
	smi	(a0)
	spl	(a0)
	svs	(a0)
	svc	(a0)
	shi	(a0)
	sls	(a0)

* ─── Scc with other addressing modes ────────────────────────────────────────
	seq	(a0)+
	sne	-(a0)
	slt	4(a0)
