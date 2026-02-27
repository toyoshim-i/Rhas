* Symbol pseudo-instruction tests
* .equ / .set / .reg
	.text

* ─── .equ: constant symbol definition ───────────────────────────────────────
CONST	.equ	42
SIZE	.equ	0x100
NEG	.equ	-1
MAX	.equ	0x7FFFFFFF

* use the constants
	moveq	#CONST,d0
	move.w	#SIZE,d1
	moveq	#NEG,d2
	move.l	#MAX,d3

* ─── .equ with expressions ──────────────────────────────────────────────────
EXPR1	.equ	CONST+SIZE
EXPR2	.equ	SIZE*2
EXPR3	.equ	CONST<<2

	move.w	#EXPR1,d0
	move.w	#EXPR2,d1
	move.l	#EXPR3,d2

* ─── .set: redefinable symbol ────────────────────────────────────────────────
COUNTER	.set	0
	moveq	#COUNTER,d0
COUNTER	.set	COUNTER+1
	moveq	#COUNTER,d1
COUNTER	.set	COUNTER+1
	moveq	#COUNTER,d2

* ─── .reg: register list symbol ──────────────────────────────────────────────
SAVED_REGS	.reg	d3-d7/a2-a6

	movem.l	SAVED_REGS,-(sp)
	movem.l	(sp)+,SAVED_REGS
