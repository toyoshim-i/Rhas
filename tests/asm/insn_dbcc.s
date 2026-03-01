* DBcc instruction variants
	.text
loop:
	nop
* All conditions
	dbra	d0,loop
	dbf	d1,loop
	dbt	d2,loop
	dbhi	d3,loop
	dbls	d4,loop
	dbcc	d5,loop
	dbcs	d6,loop
	dbne	d7,loop
	dbeq	d0,loop
	dbvc	d1,loop
	dbvs	d2,loop
	dbpl	d3,loop
	dbmi	d4,loop
	dbge	d5,loop
	dblt	d6,loop
	dbgt	d7,loop
	dble	d0,loop
	.end
