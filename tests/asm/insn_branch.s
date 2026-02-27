* Branch and control flow instruction tests
* BRA / BSR / Bcc / JMP / JSR / RTS / RTR / RTE / DBCC
	.text

* ─── unconditional branch (backward = short) ─────────────────────────────────
bra_tgt	nop
	bra	bra_tgt
	bra.s	bra_tgt

* ─── bsr (backward = short) ──────────────────────────────────────────────────
sub1	rts
	bsr	sub1
	bsr.s	sub1

* ─── conditional branches, all conditions (backward → .s) ───────────────────
* one common backward target
back	nop
	beq	back
	bne	back
	blt	back
	bgt	back
	ble	back
	bge	back
	bcc	back
	bcs	back
	bmi	back
	bpl	back
	bvs	back
	bvc	back
	bhi	back
	bls	back

* ─── explicit .s and .w forms ────────────────────────────────────────────────
bfwd	nop
	beq.s	bfwd
	bne.w	bfwd
	blt.w	bfwd
	bge.w	bfwd

* ─── jmp / jsr ───────────────────────────────────────────────────────────────
	jmp	(a0)
	jmp	4(a0)
	jmp	0(a0,d0.w)
	jsr	(a0)
	jsr	4(a0)
	jsr	0(a0,d0.w)

* ─── return instructions ──────────────────────────────────────────────────────
	rts
	rtr
	rte

* ─── dbcc ────────────────────────────────────────────────────────────────────
dbloop	nop
	dbra	d0,dbloop
	dbf	d0,dbloop
	dbt	d0,dbloop
	dbeq	d0,dbloop
	dbne	d0,dbloop
	dblt	d0,dbloop
	dbgt	d0,dbloop
	dble	d0,dbloop
	dbge	d0,dbloop
	dbcc	d0,dbloop
	dbcs	d0,dbloop
	dbmi	d0,dbloop
	dbpl	d0,dbloop
	dbvs	d0,dbloop
	dbvc	d0,dbloop
	dbhi	d0,dbloop
	dbls	d0,dbloop
