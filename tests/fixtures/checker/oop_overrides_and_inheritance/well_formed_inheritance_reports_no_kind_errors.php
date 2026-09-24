<?php
            interface OkI {}
            interface OkJ extends OkI {}
            trait OkT {}
            abstract class OkBase implements OkJ {}
            class OkSub extends OkBase { use OkT; }
            enum OkE implements OkI { use OkT; }
        