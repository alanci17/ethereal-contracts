use scrypto::prelude::*;
use scrypto_math::*;

#[blueprint]
mod tri {
  enable_method_auth! {
    roles {
      azero => updatable_by: [];
    },
    methods {
      to_nothing => restrict_to: [azero];
      first_deposit => restrict_to: [azero];
      start_stop => restrict_to: [azero];
      add_liquidity => PUBLIC;
      in_given_out => PUBLIC;
      in_given_price => PUBLIC;
      remove_liquidity => PUBLIC;
      sim_swap => PUBLIC;
      spot_price => PUBLIC;
      swap => PUBLIC;
      vault_reserves => PUBLIC;
      look_within => PUBLIC;
    }
  }

  struct Tri {
    alpha_addr: ComponentAddress,

    power_tri: Vault,

    resources: ((ResourceAddress, Decimal), (ResourceAddress, Decimal)),
    pool: ComponentAddress,
    swap_fee: Decimal,
    stopped: bool 
  }

  impl Tri {
    // instantiates the TriPool, 
    // starting it as 'stopped'
    pub fn from_nothing(alpha_addr: ComponentAddress, 
      power_azero: ResourceAddress,
      power_tri: Bucket, 
      t1: ResourceAddress, t1w: Decimal, t2: ResourceAddress, t2w: Decimal,
      swap_fee: Decimal, bang: ComponentAddress )-> ComponentAddress {
     
      assert!( t1w + t2w == dec!("1") && t1w > dec!("0") && t2w > dec!("0"), 
        "weights must sum to 1 and both be positive");
      
      assert!( swap_fee <= dec!("1") && swap_fee >= dec!("0.9"), 
        "fee must be smaller than 10% and positive");

      let pool: Global<TwoResourcePool> = Blueprint::<TwoResourcePool>::instantiate(
        // using power tri cause it's super annoying to tie it to power alpha (and use it here)
        OwnerRole::Fixed(rule!(require(power_tri.resource_address()))),
        rule!(require(power_tri.resource_address())),
        (t1, t2),
        None
      );

      let power_tri = Vault::with_bucket(power_tri);

      let lp_ga: GlobalAddress = pool.get_metadata("pool_unit")
        .expect("incoherence").expect("incoherence"); // :^)

      // yes, this is the best way afaik lmao
      let lp_ra = ResourceAddress::new_or_panic(Into::<[u8; 30]>::into(lp_ga));

      power_tri.as_fungible().authorize_with_amount(dec!(1), || {
        let rm = ResourceManager::from(lp_ra);

        rm.set_metadata(
          "name",
          "Ethereal TLP".to_owned()
        );
        rm.set_metadata(
          "symbol",
          "ETLP".to_owned()
        );
        rm.set_metadata(
          "tags",
          vec!["ethereal-dao".to_owned(), "lp".to_owned()]
        );
        rm.set_metadata(
          "info_url",
          Url::of("https://ethereal.systems")
        );
        rm.set_metadata(
          "icon_url",
          Url::of("https://cdn.discordapp.com/attachments/1092987092864335884/1095874817758081145/logos1.jpeg")
        );
        rm.set_metadata(
          "dapp_definitions",
          vec!(GlobalAddress::from(bang))
        );

        pool.set_metadata(
          "dapp_definition",
          GlobalAddress::from(bang)
        );
      });

      let a1 = Self {
        alpha_addr,
        power_tri,
        resources: ((t1, t1w), (t2, t2w)),
        pool: pool.address(),
        swap_fee,
        stopped: true
      }
      .instantiate()
      .prepare_to_globalize(OwnerRole::None)
      .roles(
        roles!(
          azero => rule!(require(power_azero));
        )
      )
      .metadata(
        metadata!(
          roles {
            metadata_setter => rule!(require(power_azero));
            metadata_setter_updater => rule!(deny_all);
            metadata_locker => rule!(deny_all);
            metadata_locker_updater => rule!(deny_all);
          },
          init {
            "dapp_definition" =>
              GlobalAddress::from(bang), updatable;
            "tags" => vec!["ethereal-dao".to_owned(), 
              "tri".to_owned()], updatable;
          }
        )
      )
      .globalize()
      .address();

      return a1
    }

    // AuthRule: power_zero
    // rips the soul out
    // the TriPool's TLP is managed by the native component
    // so the liquidity is left alone
    pub fn to_nothing(&mut self) -> Bucket {
      self.power_tri.take_all()
    }

    pub fn look_within(&self) -> 
      (
        ((ResourceAddress, Decimal), (ResourceAddress, Decimal)),
        ComponentAddress,
        Decimal,
        bool
      )
      {
      (
        self.resources,
        self.pool,
        self.swap_fee,
        self.stopped
      )
    }

    // separated from instantiation for dao reasons
    // separateed from add_liquidity for efficiency reasons
    pub fn first_deposit(&mut self, b1: Bucket, b2: Bucket) -> (Bucket, Option<Bucket>) {
      assert!( *self.vault_reserves().iter().next().expect("incoherence").1 == dec!(0),
        "first deposit into an already running pool");

      let mut pool: Global<TwoResourcePool> = self.pool.into();

      self.power_tri.as_fungible().authorize_with_amount(dec!(1), ||
        pool.contribute((b1, b2))
      )
    }

    // AuthRule: power_alpha
    // full start full stop
    pub fn start_stop(&mut self, input: bool) {
      self.stopped = input;
    }
    // TODO HALT ALL ACTIONS WHEN STOPPED

    // adds all three, basing it on the REAL deposit for correct proportion
    // does not return excess liquidity, just 'swap-balances' them out
    pub fn add_liquidity(&mut self, b1: Bucket, b2: Bucket) -> (Bucket, Option<Bucket>) {
      assert!( !self.stopped && !self.power_tri.is_empty(),
        "DEX stopped or empty"); 

      let mut pool: Global<TwoResourcePool> = self.pool.into();

      self.power_tri.as_fungible().authorize_with_amount(dec!(1), ||
        pool.contribute((b1, b2))
      )
    }

    pub fn remove_liquidity(&mut self, input: Bucket) -> (Bucket, Bucket) {
      assert!( !self.stopped && !self.power_tri.is_empty(),
        "DEX stopped or empty");

      let mut pool: Global<TwoResourcePool> = self.pool.into();

      pool.redeem(input)
    }

    // no slippage limit, can set it in the manifest
    pub fn swap(&mut self, input: Bucket) -> Bucket {
      assert!( !self.stopped && !self.power_tri.is_empty(),
        "DEX stopped or empty"); 

      let mut pool: Global<TwoResourcePool> = self.pool.into();

      let size_in = input.amount() * self.swap_fee;
      let ra_in = input.resource_address();

      // block transferred to 'swap_size' method to share swap size calc with 'sim_swap' method 
      let (ra_out, size_out) = self.swap_size(ra_in, size_in);

      self.power_tri.as_fungible().authorize_with_amount(dec!(1), || {
        pool.protected_deposit(input);
        pool.protected_withdraw(ra_out, size_out, 
          WithdrawStrategy::Rounded(RoundingMode::ToZero))
      })
    }
    
    // AUXILIARY (for interop)

    // how many to input to get a set number on output? 
    pub fn in_given_out(&self, size_out: Decimal, ra_in: ResourceAddress) -> Decimal {
      let reserves = self.vault_reserves();

      let (ra_out, w_out) = if ra_in == self.resources.0.0 {
        self.resources.1
      } else if ra_in == self.resources.1.0 {
        self.resources.0
      } else {
        panic!("wrong resource input")
      };

      let reserves_out = reserves.get(&ra_out).expect("coherence error");
      let reserves_in = reserves.get(&ra_in).expect("coherence error");

      (*reserves_in * (
        (*reserves_out / (*reserves_out - size_out))
        .pow(w_out / (dec!("1") - w_out)).expect("power incoherence") 
        - dec!("1"))
      ) / self.swap_fee
    }

    // how many to input to push it to target price?
    // returns None, if target < spot
    pub fn in_given_price(&self, target: Decimal, ra_in: ResourceAddress) -> Option<Decimal> {
      let reserves = self.vault_reserves();

      let (spot_price,w_out) = if ra_in == self.resources.0.0 {
        (
          ((*reserves.get(&self.resources.0.0).expect("incoherence") / self.resources.0.1)
          /
          (*reserves.get(&self.resources.1.0).expect("incoherence") / self.resources.1.1)),
          self.resources.1.1
        )
      } else if ra_in == self.resources.1.0 {
        (
          ((*reserves.get(&self.resources.1.0).expect("incoherence") / self.resources.1.1)
          /
          (*reserves.get(&self.resources.0.0).expect("incoherence") / self.resources.0.1)),
          self.resources.0.1
        )
      } else {
        panic!("wrong resource input")
      };
      
      let reserves_in = reserves.get(&ra_in).expect("coherence error");

      if target > spot_price {
        Some(  
          (*reserves_in * (
            (target / spot_price)
            .pow(w_out).expect("power incoherence") 
            - dec!("1"))) 
          / self.swap_fee
        )
      } else {
        None
      }
    }

    // dumps current # of in each bucket
    pub fn vault_reserves(&self) -> IndexMap<ResourceAddress, Decimal> {
      let pool: Global<TwoResourcePool> = self.pool.into();

      pool.get_vault_amounts()
    }
    
    // lookup spot price between the assets
    // todo check if it's REAL/EUXLP or the other way round
    pub fn spot_price(&self) -> Decimal {
      let reserves = self.vault_reserves();

      ((*reserves.get(&self.resources.0.0).expect("incoherence") / self.resources.0.1)
      /
      (*reserves.get(&self.resources.1.0).expect("incoherence") / self.resources.1.1))
      *
      (dec!("1") / self.swap_fee)
    }

    // simulated swap, returns the amount that will be returned with a regular swap
    pub fn sim_swap(&mut self, input: Decimal, resource_in: ResourceAddress) -> Decimal {
      let (_ra_out, size_out) = self.swap_size(resource_in, input * self.swap_fee);

      size_out
    }

    // 'out_given_in' and 'sim_swap' methods swap size calculation
    fn swap_size(&mut self, ra_in: ResourceAddress, size_in: Decimal) -> (ResourceAddress,Decimal) {
      let reserves = self.vault_reserves();

      let (ra_out, w_out) = if ra_in == self.resources.0.0 {
        self.resources.1
      } else if ra_in == self.resources.1.0 {
        self.resources.0
      } else {
        panic!("wrong resource input")
      };

      let reserves_out = reserves.get(&ra_out).expect("coherence error");
      let reserves_in = reserves.get(&ra_in).expect("coherence error");
      
      (
        ra_out, 
        *reserves_out * (dec!("1") - 
          (*reserves_in / (*reserves_in + size_in))
            .pow((dec!("1") - w_out) / w_out).expect("power incoherence") 
          )
      )
    }  
  }
}
