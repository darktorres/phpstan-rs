<?php
            /** @phpstan-sealed ChildA */
            abstract class Shape {}
            class ChildA extends Shape {}
            class GrandChild extends ChildA {}
        