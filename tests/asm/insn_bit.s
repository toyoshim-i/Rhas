* Bit manipulation instruction tests (BTST / BSET / BCLR / BCHG)
	.text

* ─── btst ────────────────────────────────────────────────────────────────────
* dynamic (Dn register)
	btst	d0,d1
	btst	d7,d0
	btst	d0,(a0)
	btst	d0,(a0)+
	btst	d0,-(a0)
* static (immediate)
	btst	#0,d0
	btst	#7,d0
	btst	#31,d0
	btst	#0,(a0)
	btst	#7,(a0)

* ─── bset ────────────────────────────────────────────────────────────────────
* dynamic
	bset	d0,d1
	bset	d7,d0
	bset	d0,(a0)
	bset	d0,(a0)+
	bset	d0,-(a0)
* static
	bset	#0,d0
	bset	#7,d0
	bset	#31,d0
	bset	#0,(a0)
	bset	#7,(a0)

* ─── bclr ────────────────────────────────────────────────────────────────────
* dynamic
	bclr	d0,d1
	bclr	d7,d0
	bclr	d0,(a0)
	bclr	d0,(a0)+
	bclr	d0,-(a0)
* static
	bclr	#0,d0
	bclr	#7,d0
	bclr	#31,d0
	bclr	#0,(a0)
	bclr	#7,(a0)

* ─── bchg ────────────────────────────────────────────────────────────────────
* dynamic
	bchg	d0,d1
	bchg	d7,d0
	bchg	d0,(a0)
	bchg	d0,(a0)+
	bchg	d0,-(a0)
* static
	bchg	#0,d0
	bchg	#7,d0
	bchg	#31,d0
	bchg	#0,(a0)
	bchg	#7,(a0)
