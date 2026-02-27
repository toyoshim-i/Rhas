* Logic instruction tests (AND / OR / EOR / NOT / ANDI / ORI / EORI)
	.text

* ─── and ─────────────────────────────────────────────────────────────────────
	and.b	d0,d1
	and.b	d1,d0
	and.b	(a0),d0
	and.b	d0,(a0)
	and.b	#0x0F,d0
	and.w	d0,d1
	and.w	(a0),d0
	and.w	d0,(a0)
	and.w	#0xFF00,d0
	and.l	d0,d1
	and.l	(a0),d0
	and.l	d0,(a0)
	and.l	#0x00FF00FF,d0

* ─── andi ────────────────────────────────────────────────────────────────────
	andi.b	#0x0F,d0
	andi.b	#0xFF,(a0)
	andi.w	#0xFF00,d0
	andi.l	#0x00FF00FF,d0
	andi.b	#0xFE,ccr
	andi.w	#0xF8FF,sr

* ─── or ──────────────────────────────────────────────────────────────────────
	or.b	d0,d1
	or.b	d1,d0
	or.b	(a0),d0
	or.b	d0,(a0)
	or.b	#0x01,d0
	or.w	d0,d1
	or.w	(a0),d0
	or.w	d0,(a0)
	or.l	d0,d1
	or.l	(a0),d0
	or.l	d0,(a0)

* ─── ori ─────────────────────────────────────────────────────────────────────
	ori.b	#0x01,d0
	ori.b	#0x01,(a0)
	ori.w	#0x0001,d0
	ori.l	#0x00000001,d0
	ori.b	#0x01,ccr
	ori.w	#0x0700,sr

* ─── eor ─────────────────────────────────────────────────────────────────────
	eor.b	d0,d1
	eor.b	d0,(a0)
	eor.w	d0,d1
	eor.w	d0,(a0)
	eor.l	d0,d1
	eor.l	d0,(a0)

* ─── eori ────────────────────────────────────────────────────────────────────
	eori.b	#0xFF,d0
	eori.b	#0xFF,(a0)
	eori.w	#0x00FF,d0
	eori.l	#0x0000FFFF,d0
	eori.b	#0x01,ccr
	eori.w	#0x0700,sr

* ─── not ─────────────────────────────────────────────────────────────────────
	not.b	d0
	not.w	d0
	not.l	d0
	not.b	(a0)
	not.w	(a0)
	not.l	(a0)
