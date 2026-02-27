* Shift and rotate instruction tests
* ASL / ASR / LSL / LSR / ROL / ROR / ROXL / ROXR
	.text

* ─── asl ─────────────────────────────────────────────────────────────────────
* immediate count form
	asl.b	#1,d0
	asl.b	#4,d0
	asl.b	#8,d0
	asl.w	#1,d0
	asl.w	#4,d0
	asl.l	#1,d0
	asl.l	#8,d0
* register count form
	asl.b	d0,d1
	asl.w	d0,d1
	asl.l	d0,d1
	asl.b	d7,d0
* memory form (shift by 1)
	asl.w	(a0)
	asl.w	(a0)+
	asl.w	-(a0)
	asl.w	4(a0)

* ─── asr ─────────────────────────────────────────────────────────────────────
* immediate count form
	asr.b	#1,d0
	asr.b	#4,d0
	asr.b	#8,d0
	asr.w	#1,d0
	asr.w	#4,d0
	asr.l	#1,d0
	asr.l	#8,d0
* register count form
	asr.b	d0,d1
	asr.w	d0,d1
	asr.l	d0,d1
* memory form
	asr.w	(a0)
	asr.w	(a0)+
	asr.w	-(a0)

* ─── lsl ─────────────────────────────────────────────────────────────────────
* immediate count form
	lsl.b	#1,d0
	lsl.b	#4,d0
	lsl.b	#8,d0
	lsl.w	#1,d0
	lsl.w	#4,d0
	lsl.l	#1,d0
	lsl.l	#8,d0
* register count form
	lsl.b	d0,d1
	lsl.w	d0,d1
	lsl.l	d0,d1
* memory form
	lsl.w	(a0)
	lsl.w	(a0)+
	lsl.w	-(a0)
	lsl.w	4(a0)

* ─── lsr ─────────────────────────────────────────────────────────────────────
* immediate count form
	lsr.b	#1,d0
	lsr.b	#4,d0
	lsr.b	#8,d0
	lsr.w	#1,d0
	lsr.w	#4,d0
	lsr.l	#1,d0
	lsr.l	#8,d0
* register count form
	lsr.b	d0,d1
	lsr.w	d0,d1
	lsr.l	d0,d1
* memory form
	lsr.w	(a0)
	lsr.w	(a0)+
	lsr.w	-(a0)

* ─── rol ─────────────────────────────────────────────────────────────────────
* immediate count form
	rol.b	#1,d0
	rol.b	#4,d0
	rol.b	#8,d0
	rol.w	#1,d0
	rol.l	#1,d0
* register count form
	rol.b	d0,d1
	rol.w	d0,d1
	rol.l	d0,d1
* memory form
	rol.w	(a0)
	rol.w	(a0)+
	rol.w	-(a0)

* ─── ror ─────────────────────────────────────────────────────────────────────
* immediate count form
	ror.b	#1,d0
	ror.b	#4,d0
	ror.b	#8,d0
	ror.w	#1,d0
	ror.l	#1,d0
* register count form
	ror.b	d0,d1
	ror.w	d0,d1
	ror.l	d0,d1
* memory form
	ror.w	(a0)
	ror.w	(a0)+
	ror.w	-(a0)

* ─── roxl ────────────────────────────────────────────────────────────────────
* immediate count form
	roxl.b	#1,d0
	roxl.b	#4,d0
	roxl.b	#8,d0
	roxl.w	#1,d0
	roxl.l	#1,d0
* register count form
	roxl.b	d0,d1
	roxl.w	d0,d1
	roxl.l	d0,d1
* memory form
	roxl.w	(a0)
	roxl.w	(a0)+
	roxl.w	-(a0)

* ─── roxr ────────────────────────────────────────────────────────────────────
* immediate count form
	roxr.b	#1,d0
	roxr.b	#4,d0
	roxr.b	#8,d0
	roxr.w	#1,d0
	roxr.l	#1,d0
* register count form
	roxr.b	d0,d1
	roxr.w	d0,d1
	roxr.l	d0,d1
* memory form
	roxr.w	(a0)
	roxr.w	(a0)+
	roxr.w	-(a0)
