* Section pseudo-instruction tests
* .text / .data / .bss / .stack
	.text
	move.b	d0,d1
	move.w	d1,d2
	nop

	.data
	.dc.w	0x1234
	.dc.l	0x12345678
	.dc.b	0x41,0x42,0x43

	.bss
	.ds.b	4
	.ds.w	2
	.ds.l	1

* ─── switch back to text ──────────────────────────────────────────────────────
	.text
	add.b	d0,d1
	nop

* ─── multiple section switches ───────────────────────────────────────────────
	.data
	.dc.b	0xDE,0xAD,0xBE,0xEF

	.text
	clr.l	d0

	.data
	.dc.w	0xCAFE

	.bss
	.ds.l	4

* ─── .stack section ──────────────────────────────────────────────────────────
	.stack
	.ds.l	64
