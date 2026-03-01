* Bit field instructions (68020+)
	.cpu	68020
	.text
* BFTST
	bftst	d0{0:8}
	bftst	d1{4:16}
	bftst	(a0){0:32}
	bftst	d2{d3:d4}
* BFSET
	bfset	d0{0:8}
	bfset	(a0){8:24}
* BFCLR
	bfclr	d0{0:8}
	bfclr	(a1){0:16}
* BFCHG
	bfchg	d0{0:8}
	bfchg	(a0){4:12}
* BFEXTU
	bfextu	d0{0:8},d1
	bfextu	(a0){0:16},d2
* BFEXTS
	bfexts	d0{0:8},d3
	bfexts	(a0){0:32},d4
* BFFFO
	bfffo	d0{0:32},d5
	bfffo	(a0){8:8},d6
* BFINS
	bfins	d0,d1{0:8}
	bfins	d2,(a0){0:16}
	.end
