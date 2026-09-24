<?php
            abstract class Base {
                abstract public int $noHooks;
                abstract public int $allBodies { get => 1; }
                public int $bodiless { get; }
                abstract private int $absPriv { get; }
                final private int $finPriv = 1;
                private int $finHook { final get => 1; }
                abstract public int $absFinalHook { final get; }
                public readonly int $roHooked { get => 1; }
                public static int $staticHooked { get => 1; }
                public int $virtual = 3 { get => 1; }
                public ?int $fine { get => $this->fine; set => $value; }
            }
            class Concrete {
                abstract public int $abs { get; }
            }
        