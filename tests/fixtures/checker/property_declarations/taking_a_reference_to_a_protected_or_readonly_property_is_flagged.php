<?php
            class Refs {
                public private(set) int $asym = 0;
                protected int $prot = 0;
                public readonly int $ro;
                /** @readonly */
                public int $docRo = 0;
                /** @readonly */
                public static int $docStatic = 0;
                public int $open = 0;
                public function __construct() { $this->ro = 1; }
                public function inside(): void {
                    $a = &$this->ro;
                    $b = &$this->docRo;
                    $c = &self::$docStatic;
                    $d = &$this->asym;
                }
            }
            function f(Refs $r): void {
                $a = &$r->asym;
                $b = &$r->prot;
                $c = &$r->open;
            }
        