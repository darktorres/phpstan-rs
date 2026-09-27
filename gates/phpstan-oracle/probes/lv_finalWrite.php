<?php
final class FP {}
function fp(FP $p): void { $p->x = 1; echo $p->y; }
