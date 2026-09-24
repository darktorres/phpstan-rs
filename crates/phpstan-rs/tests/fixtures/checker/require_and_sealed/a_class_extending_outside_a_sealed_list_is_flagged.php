<?php
            /** @phpstan-sealed ChildA|ChildB */
            abstract class Shape {}
            class ChildA extends Shape {}
            class ChildB extends Shape {}
            class ChildC extends Shape {}
        