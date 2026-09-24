<?php
            interface Shape {
                public int $plain;
                protected int $hidden { get; }
                public int $ok { get; }
                public readonly int $ro { get; }
                public static int $st { get; }
                public final int $fin { get; }
                public int $body { get => 1; }
                public int $finalHook { final get; }
            }
        